//! Somewhere to look after omt disappears.
//!
//! A terminal that dies mid-frame is the worst thing to diagnose, because the
//! evidence destroys itself: the panic goes to stderr, stderr is on the
//! alternate screen, and leaving the alternate screen throws that screen away.
//! What the user is left with is their shell, no cursor, and nothing to report.
//!
//! So the note goes to a file, and the terminal is put back *before* the
//! message is written — in that order, because a message written into a screen
//! that is about to be discarded is a message nobody reads.

use std::io::Write;

/// Where the note goes.
///
/// A fixed name rather than one per run: somebody who has just watched their
/// terminal vanish should not have to work out which file is theirs, and the
/// last crash is the one being asked about.
#[must_use]
pub fn note_path() -> std::path::PathBuf {
    std::env::temp_dir().join("omt-crash.log")
}

/// Put the terminal back and write down what happened, on the way down.
pub fn leave_a_note() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        // First, hand the terminal back. Raw mode with no cursor and a half
        // drawn frame is not something to leave somebody in while writing a
        // log file.
        let mut out = std::io::stdout();
        let _ = out.write_all(
            b"\x1b[?1006l\x1b[?1003l\x1b[?1002l\x1b[?1000l\x1b[?2004l\x1b[?25h\x1b[?1049l",
        );
        let _ = out.flush();
        let _ = crossterm::terminal::disable_raw_mode();

        let where_ = panic
            .location()
            .map(|at| format!("{}:{}:{}", at.file(), at.line(), at.column()));
        let what = panic
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| panic.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "a panic with nothing to say".to_owned());
        let note = format!(
            "omt {} ({}) panicked\n  where: {}\n  what:  {}\n",
            env!("CARGO_PKG_VERSION"),
            env!("OMT_BUILD_COMMIT"),
            where_.unwrap_or_else(|| "unknown".to_owned()),
            what,
        );

        // The file first, because it is the copy that survives.
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(note_path())
        {
            let _ = file.write_all(note.as_bytes());
        }
        // And on the screen, which is now a screen somebody can read.
        eprint!("{note}");
        eprintln!("  also written to {}", note_path().display());
        previous(panic);
    }));
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    #[test]
    fn the_note_has_one_fixed_place_to_look() {
        // Somebody who has just watched their terminal vanish should not have
        // to work out which file is theirs.
        assert_eq!(note_path(), note_path());
        assert!(note_path().ends_with("omt-crash.log"));
    }
}
