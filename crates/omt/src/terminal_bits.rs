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

use crate::panes::LocalPane;
use crate::rows::agent_name;

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

/// A configured number, where one is set and sane.
///
/// `None` for absent, unreadable or nonsensical, all three deliberately: the
/// caller's fallback is the built-in default, which is a real answer. A
/// terminal that refused to start over a settings file is worse than one with
/// default settings.
pub(crate) fn number_setting(state: &crate::state::State, key: &str) -> Option<u64> {
    state
        .config()
        .ok()
        .and_then(|resolved| resolved.get(key).and_then(serde_json::Value::as_u64))
        .filter(|n| *n > 0)
}

/// The scrollback caps this instance is configured with.
///
/// `terminal.scrollback_lines` has been in the settings reference, the settings
/// screen and the importer since any of them existed, and **it never reached
/// `ScrollbackLimits`** — every session kept the built-in ten thousand lines
/// while the settings screen reported whatever the user had asked for. A
/// setting that is accepted and does nothing is worse than one that is
/// refused.
pub(crate) fn scrollback_limits(state: &crate::state::State) -> omt_term::ScrollbackLimits {
    scrollback_limits_of(number_setting(state, "terminal.scrollback_lines"))
}

/// The same, from the number itself.
///
/// Split from the lookup so it can be checked without a configuration on disk:
/// a test that reads the real files passes or fails according to the settings
/// of whoever runs it, which is a test of their machine.
pub(crate) fn scrollback_limits_of(lines: Option<u64>) -> omt_term::ScrollbackLimits {
    let mut limits = omt_term::ScrollbackLimits::default();
    if let Some(lines) = lines {
        limits.max_lines = u32::try_from(lines).unwrap_or(u32::MAX);
    }
    limits
}

/// The shell a new pane starts, in the order the documents promise.
///
/// `terminal.default_shell` before `$SHELL`: the setting is the one thing a
/// user set on purpose, and it was read by nothing at all.
pub(crate) fn default_shell(state: &crate::state::State) -> Option<String> {
    let configured = setting(state, "terminal.default_shell");
    let trimmed = configured.trim().trim_matches('"').trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CopyKey {
    /// Move.
    Go(omt_tui::CopyMove),
    /// Start or clear a selection.
    Mark,
    /// Take a copy and leave.
    Yank,
    /// Open the search field, forwards or backwards.
    Search(bool),
    /// Move by word.
    Word(omt_tui::CopyWord),
    /// Start a whole-line selection.
    MarkLine,
    /// To the previous or next prompt.
    Prompt(bool),
    /// Repeat the last search, forwards or backwards.
    Repeat(bool),
    /// Leave without copying.
    Leave,
    /// Nothing. An unbound key does nothing rather than reaching the shell:
    /// a keystroke that fell through to a program nobody is looking at is how
    /// a reader ends up having typed into their own session.
    Ignore,
}

/// Which key table copy mode is using.
///
/// Read from the binding file's `keymap`, which is where 10 §8.2 puts it. It
/// was read from the *configuration* stack for a while, where the key does not
/// exist: the merge answered `OMT-C101 unknown key`, so the flavour was always
/// the default and anybody who set it got an error rather than emacs motions.
/// Reading a key from the wrong store fails exactly like reading nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Flavour {
    /// vim's motions, which is what copy mode has always used.
    #[default]
    Vim,
    /// emacs' motions, for the fingers that have those instead.
    Emacs,
}

impl Flavour {
    /// Read it off the setting.
    ///
    /// Anything unrecognised is the default rather than an error: a keymap
    /// name omt does not ship is a typo or a file from a later version, and
    /// neither is worth refusing to open a terminal over.
    pub(crate) fn named(name: &str) -> Self {
        match name {
            "emacs" => Self::Emacs,
            _ => Self::Vim,
        }
    }
}

/// The keys, which are the ones people already have in their fingers.
pub(crate) fn copy_key(key: &crossterm::event::KeyEvent, flavour: Flavour) -> CopyKey {
    use crossterm::event::{KeyCode, KeyModifiers};
    use omt_tui::CopyMove as Go;
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);

    // Shared by both tables: the keys that are not a motion vocabulary at all.
    // An arrow key means the same thing to everybody.
    match key.code {
        KeyCode::Left => return CopyKey::Go(Go::Left),
        KeyCode::Down => return CopyKey::Go(Go::Down),
        KeyCode::Up => return CopyKey::Go(Go::Up),
        KeyCode::Right => return CopyKey::Go(Go::Right),
        KeyCode::Home => return CopyKey::Go(Go::LineStart),
        KeyCode::End => return CopyKey::Go(Go::LineEnd),
        KeyCode::PageUp => return CopyKey::Go(Go::PageUp),
        KeyCode::PageDown => return CopyKey::Go(Go::PageDown),
        KeyCode::Esc => return CopyKey::Leave,
        KeyCode::Enter => return CopyKey::Yank,
        _ => {}
    }

    match flavour {
        Flavour::Vim => match key.code {
            KeyCode::Char('d') if ctrl => CopyKey::Go(Go::HalfPageDown),
            KeyCode::Char('u') if ctrl => CopyKey::Go(Go::HalfPageUp),
            KeyCode::Char('h') => CopyKey::Go(Go::Left),
            KeyCode::Char('j') => CopyKey::Go(Go::Down),
            KeyCode::Char('k') => CopyKey::Go(Go::Up),
            KeyCode::Char('l') => CopyKey::Go(Go::Right),
            KeyCode::Char('0') => CopyKey::Go(Go::LineStart),
            KeyCode::Char('^') => CopyKey::Go(Go::FirstNonBlank),
            KeyCode::Char('$') => CopyKey::Go(Go::LineEnd),
            KeyCode::Char('w') => CopyKey::Word(omt_tui::CopyWord::Next),
            KeyCode::Char('b') if !ctrl => CopyKey::Word(omt_tui::CopyWord::Back),
            KeyCode::Char('e') => CopyKey::Word(omt_tui::CopyWord::End),
            KeyCode::Char('g') => CopyKey::Go(Go::Top),
            KeyCode::Char('G') => CopyKey::Go(Go::Bottom),
            KeyCode::Char('b') => CopyKey::Go(Go::PageUp),
            KeyCode::Char('f') => CopyKey::Go(Go::PageDown),
            KeyCode::Char('v') | KeyCode::Char(' ') => CopyKey::Mark,
            KeyCode::Char('V') => CopyKey::MarkLine,
            KeyCode::Char('[') => CopyKey::Prompt(false),
            KeyCode::Char(']') => CopyKey::Prompt(true),
            KeyCode::Char('/') => CopyKey::Search(true),
            KeyCode::Char('?') => CopyKey::Search(false),
            KeyCode::Char('n') => CopyKey::Repeat(true),
            KeyCode::Char('N') => CopyKey::Repeat(false),
            KeyCode::Char('y') => CopyKey::Yank,
            KeyCode::Char('q') => CopyKey::Leave,
            _ => CopyKey::Ignore,
        },
        Flavour::Emacs => match key.code {
            KeyCode::Char('b') if ctrl => CopyKey::Go(Go::Left),
            KeyCode::Char('f') if ctrl => CopyKey::Go(Go::Right),
            KeyCode::Char('p') if ctrl => CopyKey::Go(Go::Up),
            KeyCode::Char('n') if ctrl => CopyKey::Go(Go::Down),
            KeyCode::Char('a') if ctrl => CopyKey::Go(Go::LineStart),
            KeyCode::Char('e') if ctrl => CopyKey::Go(Go::LineEnd),
            KeyCode::Char('m') if alt => CopyKey::Go(Go::FirstNonBlank),
            KeyCode::Char('v') if ctrl => CopyKey::Go(Go::PageDown),
            KeyCode::Char('v') if alt => CopyKey::Go(Go::PageUp),
            KeyCode::Char('f') if alt => CopyKey::Word(omt_tui::CopyWord::Next),
            KeyCode::Char('b') if alt => CopyKey::Word(omt_tui::CopyWord::Back),
            KeyCode::Char('<') if alt => CopyKey::Go(Go::Top),
            KeyCode::Char('>') if alt => CopyKey::Go(Go::Bottom),
            // The mark, which arrives as a control space or as a null
            // depending on the terminal — both spellings, because a key that
            // works in one terminal and not another reads as a broken build.
            KeyCode::Char(' ') if ctrl => CopyKey::Mark,
            KeyCode::Char('@') if ctrl => CopyKey::Mark,
            KeyCode::Null => CopyKey::Mark,
            KeyCode::Char('l') if ctrl => CopyKey::MarkLine,
            KeyCode::Char('s') if ctrl => CopyKey::Search(true),
            KeyCode::Char('r') if ctrl => CopyKey::Search(false),
            KeyCode::Char('s') if alt => CopyKey::Repeat(true),
            KeyCode::Char('r') if alt => CopyKey::Repeat(false),
            KeyCode::Char('w') if alt => CopyKey::Yank,
            KeyCode::Char('[') if alt => CopyKey::Prompt(false),
            KeyCode::Char(']') if alt => CopyKey::Prompt(true),
            KeyCode::Char('g') if ctrl => CopyKey::Leave,
            _ => CopyKey::Ignore,
        },
    }
}

/// Paint a pane's history, with the selection marked and the way out shown.
///
/// Reverse video for the selection: it is the one highlight every terminal
/// has, in every theme, including the ones with no colour at all.
pub(crate) fn history_rows(
    reading: &omt_tui::CopyMode,
    rect: omt_tui::Rect,
    terminal: Option<&omt_term::Terminal>,
) -> Vec<(u64, String)> {
    terminal.map_or_else(Vec::new, |terminal| {
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
    })
}

/// Paint a pane's history, with the selection marked and the way out shown.
///
/// Takes the rows rather than the terminal: whichever side of the seam owns
/// that terminal, the borrow ends before the frame is written — and a
/// renderer that takes text is one a test can drive without a pty.
pub(crate) fn draw_history(
    out: &mut impl Write,
    reading: &omt_tui::CopyMode,
    rect: omt_tui::Rect,
    lines: &[(u64, String)],
    flavour: Flavour,
    palette: omt_tui::paint::Chrome,
    depth: omt_tui::paint::Depth,
) -> io::Result<()> {
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
    // The keys of whichever keymap is actually in use. A hint naming `hjkl`
    // to somebody who asked for emacs is worse than no hint: it is a list of
    // keys that do nothing, in the one place they went looking for help.
    let keys = match flavour {
        Flavour::Vim => {
            "hjkl move · w/b word · [ ] prompt · / search · v select · y copy · Esc leave"
        }
        Flavour::Emacs => {
            "^n^p^f^b move · M-f/M-b word · M-[ M-] prompt · ^s search · ^Spc select · M-w copy · ^g leave"
        }
    };
    let hint = reading.typing.as_ref().map_or_else(
        || format!("copy: {keys}"),
        // The field itself, because a search somebody cannot see what they
        // have typed into is a search they retype.
        |typed| format!("search: {typed}"),
    );
    let hint_row = rect.row + crate::run::CHROME_ROWS + rect.rows;
    write!(
        out,
        "\x1b[{hint_row};{}H{}",
        rect.col + 1,
        omt_tui::paint::hint_line(&hint, palette, depth)
    )
}

/// What one key in copy mode asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reading {
    /// Still reading.
    Stay,
    /// Done — the pane comes back.
    Leave,
    /// Copy what is selected, then leave.
    ///
    /// Returned rather than done here, because *where the lines come from* is
    /// the caller's question: a terminal that owns its session reads its own
    /// emulator, and one watching an instance asks for the span in a single
    /// call rather than a round trip per line.
    Yank,
}

/// One key, while somebody is reading history rather than typing into a pane.
///
/// Every key belongs to this mode, like an overlay and for the same reason:
/// somebody is reading, and a keystroke that reached the shell would type into
/// a program nobody is looking at.
///
/// Takes how many rows there are and a way to read one, rather than a
/// terminal: the history a reader moves through lives on whichever side of the
/// seam owns the session.
pub(crate) fn reading_key(
    key: &crossterm::event::KeyEvent,
    reading: &mut omt_tui::CopyMode,
    rect: omt_tui::Rect,
    total: u64,
    flavour: Flavour,
    // `prompts`: where the shell said its prompts are, absolute and already
    // filtered to the ones omt actually saw a command for. Filtered by the
    // caller, because that is where the block model lives and where
    // `attributed` means anything.
    prompts: &[u64],
    line: &mut dyn FnMut(u64) -> Option<String>,
) -> Reading {
    use crossterm::event::KeyCode;

    let rows = rect.rows.max(1);
    let cols = if rect.cols == 0 { 80 } else { rect.cols };

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
                    // Whichever direction the search was opened in. A search
                    // that always ran forwards would make `?` a one-shot.
                    let forwards = !reading.backwards;
                    reading.jump(forwards, total, rows, line);
                }
            }
            KeyCode::Esc => reading.typing = None,
            _ => {}
        }
        return Reading::Stay;
    }

    match copy_key(key, flavour) {
        CopyKey::Go(omt_tui::CopyMove::FirstNonBlank) => reading.first_non_blank(line),
        CopyKey::Go(towards) => reading.go(towards, total, rows, cols),
        CopyKey::Word(towards) => reading.word(towards, line),
        CopyKey::Mark => {
            reading.linewise = false;
            reading.mark();
        }
        CopyKey::MarkLine => {
            // Cleared when the selection is, so a line-wise selection that was
            // dropped does not leave the next one taking whole rows nobody
            // asked for.
            reading.mark();
            reading.linewise = reading.anchor.is_some();
        }
        CopyKey::Prompt(forwards) => {
            reading.to_prompt(forwards, prompts, rows, total);
        }
        CopyKey::Search(forwards) => {
            reading.backwards = !forwards;
            reading.search();
        }
        CopyKey::Repeat(same) => {
            // Relative to the direction the search was made in, which is what
            // `N` means.
            let forwards = if same {
                !reading.backwards
            } else {
                reading.backwards
            };
            reading.jump(forwards, total, rows, line);
        }
        CopyKey::Yank => return Reading::Yank,
        CopyKey::Leave => return Reading::Leave,
        CopyKey::Ignore => {}
    }
    Reading::Stay
}

/// How far the history goes, from whichever side owns it.
///
/// Attached, this must come from the instance: a mirror starts counting rows
/// at the moment this terminal attached, and a reader blending the two is
/// shown a line other than the one they are on.
pub(crate) fn history_depth(
    instance: &mut omt_daemon::Instance,
    attached: &mut Option<crate::backing::Attached>,
    session: Option<omt_types::SessionId>,
) -> u64 {
    if let (Some(joined), Some(id)) = (attached.as_mut(), session) {
        return crate::backing::history(joined, id, 0, 1)
            .map(|(_, total)| total)
            .unwrap_or_default();
    }
    let seam = crate::backing::backing(instance, attached);
    session
        .and_then(|id| seam.terminal(id))
        .map_or(0, omt_term::Terminal::total_rows)
}

/// One line of history, from whichever side owns it.
pub(crate) fn line_of(
    instance: &mut omt_daemon::Instance,
    attached: &mut Option<crate::backing::Attached>,
    session: Option<omt_types::SessionId>,
    row: u64,
) -> Option<String> {
    if let (Some(joined), Some(id)) = (attached.as_mut(), session) {
        return crate::backing::history_line(joined, id, row);
    }
    let seam = crate::backing::backing(instance, attached);
    session
        .and_then(|id| seam.terminal(id))
        .and_then(|t| t.line_at(row))
        .map(omt_term::Line::text)
}

/// A drag, in the reader that already knows how to hold a selection.
///
/// Reusing copy mode rather than adding a second selection is what makes a
/// mouse selection continuable with the keyboard, and what gives the pointer
/// absolute rows: a screen-relative selection moves under its own reader as
/// soon as the pane produces output.
pub(crate) fn selecting(
    copying: &mut Option<omt_tui::CopyMode>,
    kind: omt_input::mouse::Kind,
    event: &omt_input::mouse::MouseEvent,
    rect: omt_tui::Rect,
    total: u64,
    line: &mut dyn FnMut(u64) -> Option<String>,
) {
    use omt_input::mouse::Kind;
    match kind {
        Kind::Press => {
            let mut reading = omt_tui::CopyMode::entering(total, rect.rows);
            // Where the pointer is, in absolute rows: the top of the viewport
            // plus how far down the pane it was clicked.
            reading.row = reading.top.saturating_add(u64::from(event.row));
            reading.col = event.col;
            // Marked on the press, not on the first motion. A selection that
            // only starts existing once the pointer moves loses its first
            // character.
            reading.mark();
            *copying = Some(reading);
        }
        Kind::Drag => {
            if let Some(reading) = copying.as_mut() {
                reading.row = reading.top.saturating_add(u64::from(event.row));
                reading.col = event.col;
            }
        }
        Kind::Release => {
            if let Some(reading) = copying.as_ref() {
                // A press that never moved is a click, not a selection, and
                // copying on it would replace the clipboard every time
                // somebody put the cursor somewhere.
                let moved = reading
                    .anchor
                    .is_some_and(|(row, col)| (row, col) != (reading.row, reading.col));
                if moved {
                    yank(reading, line);
                }
            }
            *copying = None;
        }
        Kind::Motion => {}
    }
}

/// The wheel, over omt's own scrollback.
pub(crate) fn scrolling(
    copying: &mut Option<omt_tui::CopyMode>,
    lines: i16,
    rect: omt_tui::Rect,
    total: u64,
) {
    // Scrolling *is* reading the scrollback, so it enters the reader rather
    // than inventing a second way to look at the same rows.
    let reading = copying.get_or_insert_with(|| omt_tui::CopyMode::entering(total, rect.rows));
    let (towards, count) = if lines < 0 {
        (omt_tui::CopyMove::Up, lines.unsigned_abs())
    } else {
        (omt_tui::CopyMove::Down, lines.unsigned_abs())
    };
    for _ in 0..count {
        reading.go(towards, total, rect.rows, rect.cols);
    }
}

/// Put a selection on the clipboard.
///
/// Through the terminal omt is drawing on, which is the only clipboard
/// reachable from the far end of an ssh link — where these sessions actually
/// run.
pub(crate) fn yank(reading: &omt_tui::CopyMode, line: &mut dyn FnMut(u64) -> Option<String>) {
    let text = omt_tui::selected_text(reading, line);
    if text.is_empty() {
        return;
    }
    if let Ok(sequence) = omt_media::clipboard::osc52(
        &text,
        &std::env::var("TERM").unwrap_or_default(),
        omt_media::clipboard::DEFAULT_MAX_BYTES,
    ) {
        let mut stdout = io::stdout();
        let _ = stdout.write_all(sequence.as_bytes());
        let _ = stdout.flush();
    }
}

/// What is still running, as rows somebody reads before ending it.
///
/// Ordered by what needs a person first, for the same reason the agent list
/// is: the row that changes somebody's mind is the one at the top.
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

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn with(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    #[test]
    fn the_keymap_setting_is_finally_read() {
        // Documented in the configuration reference, parsed into the binding
        // file, and read by nothing — a setting somebody could set and watch
        // do nothing.
        assert_eq!(Flavour::named("emacs"), Flavour::Emacs);
        assert_eq!(Flavour::named("vim"), Flavour::Vim);
        assert_eq!(Flavour::named("default"), Flavour::Vim);
    }

    #[test]
    fn a_keymap_name_omt_does_not_ship_is_the_default_not_an_error() {
        // A typo, or a file written by a later version. Neither is worth
        // refusing to open a terminal over.
        assert_eq!(Flavour::named("kakoune"), Flavour::Vim);
        assert_eq!(Flavour::named(""), Flavour::Vim);
    }

    #[test]
    fn the_vim_table_has_the_motions_it_was_missing() {
        let f = Flavour::Vim;
        assert_eq!(
            copy_key(&key(KeyCode::Char('w')), f),
            CopyKey::Word(omt_tui::CopyWord::Next)
        );
        assert_eq!(
            copy_key(&key(KeyCode::Char('e')), f),
            CopyKey::Word(omt_tui::CopyWord::End)
        );
        assert_eq!(
            copy_key(&key(KeyCode::Char('^')), f),
            CopyKey::Go(omt_tui::CopyMove::FirstNonBlank)
        );
        assert_eq!(
            copy_key(&with(KeyCode::Char('d'), KeyModifiers::CONTROL), f),
            CopyKey::Go(omt_tui::CopyMove::HalfPageDown)
        );
        assert_eq!(copy_key(&key(KeyCode::Char('V')), f), CopyKey::MarkLine);
        assert_eq!(
            copy_key(&key(KeyCode::Char('?')), f),
            CopyKey::Search(false)
        );
        assert_eq!(
            copy_key(&key(KeyCode::Char('[')), f),
            CopyKey::Prompt(false)
        );
        assert_eq!(copy_key(&key(KeyCode::Char(']')), f), CopyKey::Prompt(true));
    }

    #[test]
    fn the_vim_table_still_does_what_it_did() {
        // Nothing changes for anybody who has not asked for something else.
        let f = Flavour::Vim;
        assert_eq!(
            copy_key(&key(KeyCode::Char('j')), f),
            CopyKey::Go(omt_tui::CopyMove::Down)
        );
        assert_eq!(copy_key(&key(KeyCode::Char('y')), f), CopyKey::Yank);
        assert_eq!(copy_key(&key(KeyCode::Char('q')), f), CopyKey::Leave);
        assert_eq!(copy_key(&key(KeyCode::Char('/')), f), CopyKey::Search(true));
    }

    #[test]
    fn the_emacs_table_moves_the_way_those_fingers_expect() {
        let f = Flavour::Emacs;
        let ctrl = KeyModifiers::CONTROL;
        let alt = KeyModifiers::ALT;
        assert_eq!(
            copy_key(&with(KeyCode::Char('n'), ctrl), f),
            CopyKey::Go(omt_tui::CopyMove::Down)
        );
        assert_eq!(
            copy_key(&with(KeyCode::Char('a'), ctrl), f),
            CopyKey::Go(omt_tui::CopyMove::LineStart)
        );
        assert_eq!(
            copy_key(&with(KeyCode::Char('f'), alt), f),
            CopyKey::Word(omt_tui::CopyWord::Next)
        );
        assert_eq!(
            copy_key(&with(KeyCode::Char('<'), alt), f),
            CopyKey::Go(omt_tui::CopyMove::Top)
        );
        assert_eq!(
            copy_key(&with(KeyCode::Char('s'), ctrl), f),
            CopyKey::Search(true)
        );
        assert_eq!(
            copy_key(&with(KeyCode::Char('r'), ctrl), f),
            CopyKey::Search(false)
        );
        assert_eq!(copy_key(&with(KeyCode::Char('w'), alt), f), CopyKey::Yank);
        assert_eq!(copy_key(&with(KeyCode::Char('g'), ctrl), f), CopyKey::Leave);
    }

    #[test]
    fn the_mark_arrives_spelled_more_than_one_way() {
        // Control-space reaches a program as a null in some terminals and as
        // a control space in others. A key that works in one terminal and not
        // another reads as a broken build.
        let f = Flavour::Emacs;
        assert_eq!(
            copy_key(&with(KeyCode::Char(' '), KeyModifiers::CONTROL), f),
            CopyKey::Mark
        );
        assert_eq!(copy_key(&key(KeyCode::Null), f), CopyKey::Mark);
    }

    #[test]
    fn a_vim_key_does_nothing_in_the_emacs_table() {
        // Otherwise the setting would be a superset rather than a choice, and
        // `j` would move the cursor for somebody who asked for emacs.
        assert_eq!(
            copy_key(&key(KeyCode::Char('j')), Flavour::Emacs),
            CopyKey::Ignore
        );
    }

    #[test]
    fn arrows_and_escape_mean_the_same_thing_to_everybody() {
        for flavour in [Flavour::Vim, Flavour::Emacs] {
            assert_eq!(
                copy_key(&key(KeyCode::Down), flavour),
                CopyKey::Go(omt_tui::CopyMove::Down)
            );
            assert_eq!(copy_key(&key(KeyCode::Esc), flavour), CopyKey::Leave);
            assert_eq!(copy_key(&key(KeyCode::Enter), flavour), CopyKey::Yank);
        }
    }

    #[test]
    fn a_configured_scrollback_reaches_the_limits_and_an_absent_one_does_not() {
        // It never did: `terminal.scrollback_lines` was accepted, shown with
        // its provenance, and every session kept the built-in ten thousand.
        assert_eq!(scrollback_limits_of(Some(250)).max_lines, 250);
        assert_eq!(
            scrollback_limits_of(None).max_lines,
            omt_term::ScrollbackLimits::default().max_lines
        );
        // A nonsense value is the default rather than a refusal to start.
        assert_eq!(scrollback_limits_of(Some(u64::MAX)).max_lines, u32::MAX);
    }
}
