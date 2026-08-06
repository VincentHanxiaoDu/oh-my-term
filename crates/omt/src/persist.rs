//! Keeping enough on disk that a reboot is not a loss.
//!
//! The capabilities `state.save` and `state.restore` do the work. What is here
//! is the part that makes them matter: nobody calls a save capability before
//! their laptop dies, so the daemon does it on a timer and reads it back when
//! it starts.
//!
//! What comes back is the *content* — the workspaces, the sessions, the
//! screens. Processes do not: a pty dies with the machine, and pretending
//! otherwise would be a lie a user discovers by typing into a session that
//! silently goes nowhere. Restored sessions are orphans, and they say so.

use crate::state::State;

/// How often the daemon writes what it would need to come back.
///
/// Thirty seconds. Long enough that an idle instance is not writing constantly,
/// short enough that a crash costs half a minute of context rather than an
/// afternoon's. The file is written atomically, so a crash mid-write leaves the
/// previous snapshot rather than half of a new one.
pub const AUTOSAVE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);

/// Read back what was open, if anything was.
///
/// Failures are deliberately quiet in one direction only: a missing snapshot is
/// the ordinary first run and says nothing, while one that exists and cannot be
/// read is worth a line, because it is the difference between "you had nothing"
/// and "you had something and omt could not open it".
pub fn restore_on_start(state: &State) {
    let path = match snapshot_path() {
        Some(p) => p,
        None => return,
    };
    if !path.exists() {
        return;
    }
    match crate::capabilities::restore_from(state, &path) {
        Ok(out) if out.sessions > 0 || out.workspaces > 0 => {
            println!(
                "restored {} workspace(s) and {} session(s) from before the last restart",
                out.workspaces, out.sessions
            );
            if out.sessions > 0 {
                // Said plainly, because a session that looks normal and refuses
                // every keystroke is the most confusing possible outcome.
                println!("their processes are gone — `session.restart` brings one back");
            }
        }
        Ok(_) => {}
        Err(e) => eprintln!("could not restore {}: {e}", path.display()),
    }
}

/// Write the snapshot on a timer, forever.
pub fn spawn_autosave(state: State) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(AUTOSAVE_INTERVAL);
            let Some(path) = snapshot_path() else {
                return;
            };
            // Errors are not fatal and not repeated at volume: a full disk
            // would otherwise print every thirty seconds forever.
            let _ = crate::capabilities::save_to(&state, &path);
        }
    });
}

/// Where the snapshot lives.
#[must_use]
pub fn snapshot_path() -> Option<std::path::PathBuf> {
    let base = std::env::var("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|_| {
            std::env::var("HOME").map(|h| std::path::PathBuf::from(h).join(".local/state"))
        })
        .ok()?;
    Some(base.join("omt").join("session.json"))
}
