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
    restore_on_start_for(state, None);
}

/// Read back what *this* instance had open.
pub fn restore_on_start_for(state: &State, socket: Option<&std::path::Path>) {
    let path = match snapshot_path_for(socket) {
        Some(p) => p,
        None => return,
    };
    // Whether the last omt stopped or fell over, decided before the marker is
    // rewritten for this run (05 §8.2).
    let state_dir = path.parent().map(std::path::Path::to_path_buf);
    let ended_cleanly = state_dir
        .as_deref()
        .is_none_or(crate::shutdown::ended_cleanly);
    if let Some(dir) = state_dir.as_deref() {
        // Failing to write the marker is not a reason to refuse to start; it
        // costs a crash report next time, which is worth less than the session.
        let _ = crate::shutdown::mark_running(dir);
        // And cleared when somebody stops this daemon, so an orderly stop does
        // not read as a crash on the next start.
        crate::shutdown::clear_on_signal(dir);
    }
    if !path.exists() {
        return;
    }
    if !ended_cleanly {
        // Said once, on the way in. Whatever was written after the last
        // scrollback flush is gone, and a user weighing that against what they
        // remember doing needs to be told rather than left to notice.
        eprintln!(
            "omt: the previous run did not shut down cleanly; output written just before the stop may be missing"
        );
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
    spawn_autosave_for(state, None);
}

/// Autosave *this* instance's snapshot.
pub fn spawn_autosave_for(state: State, socket: Option<&std::path::Path>) {
    let socket = socket.map(std::path::Path::to_path_buf);
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(AUTOSAVE_INTERVAL);
            let Some(path) = snapshot_path_for(socket.as_deref()) else {
                return;
            };
            // Errors are not fatal and not repeated at volume: a full disk
            // would otherwise print every thirty seconds forever.
            let _ = crate::capabilities::save_to(&state, &path);
        }
    });
}

/// Where the snapshot lives.
///
/// One file per **instance**, not per machine. It was one file per machine, and
/// two consequences followed from that:
///
/// - Two instances on one host restored each other's sessions and then
///   overwrote each other's snapshots, so whichever stopped last won and the
///   other's work was gone with nothing said.
/// - omt's own tests spawn instances, so running them while a real omt was up
///   restored the user's live sessions into a test and then saved the test's
///   state over theirs. The tests failed, which was the lucky outcome.
///
/// The instance is named by its socket, because that is what an instance *is*
/// from outside: two omts that can be told apart by a client are two omts that
/// need separate snapshots. `OMT_STATE_FILE` overrides it outright, which is
/// what a test or a service manager wants.
#[must_use]
pub fn snapshot_path_for(socket: Option<&std::path::Path>) -> Option<std::path::PathBuf> {
    if let Ok(explicit) = std::env::var("OMT_STATE_FILE") {
        return Some(std::path::PathBuf::from(explicit));
    }
    derived_snapshot_path(socket)
}

/// The path a socket derives, ignoring any override.
///
/// Split out so the derivation can be tested at all. With the override folded
/// in, a machine that happens to have `OMT_STATE_FILE` set — which is every
/// machine running omt's own end-to-end suite — makes every derived path
/// identical, and the test that two instances do not share a snapshot passes
/// or fails on the environment it was run in rather than on the code.
///
/// That is the third time in one session that an inherited environment
/// variable decided what a test proved.
#[must_use]
pub fn derived_snapshot_path(socket: Option<&std::path::Path>) -> Option<std::path::PathBuf> {
    let base = std::env::var("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|_| {
            std::env::var("HOME").map(|h| std::path::PathBuf::from(h).join(".local/state"))
        })
        .ok()?;
    let dir = base.join("omt");
    let Some(socket) = socket else {
        // No socket: a surface with no identity of its own, which is every
        // caller that existed before this took an argument.
        return Some(dir.join("session.json"));
    };
    // FNV-1a over the socket path. Short, stable, and filename-safe — the path
    // itself is not, and a snapshot called
    // `-var-folders-2x-...-T-omt-502.sock.json` is one nobody can read in a
    // directory listing.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in socket.as_os_str().as_encoded_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    Some(dir.join(format!("session-{hash:016x}.json")))
}

/// Where the default instance's snapshot lives.
#[must_use]
pub fn snapshot_path() -> Option<std::path::PathBuf> {
    snapshot_path_for(None)
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
    fn two_instances_on_one_machine_do_not_share_a_snapshot() {
        // They did. Whichever stopped last overwrote the other's, so a user
        // running two omts lost one of them silently — and omt's own tests,
        // which spawn instances, restored the user's live sessions into a test
        // and then saved the test's state over theirs.
        // The derivation, not the override: `OMT_STATE_FILE` names one file
        // for everything by design, and a machine with it set would otherwise
        // make this test pass or fail on its environment.
        let a = derived_snapshot_path(Some(std::path::Path::new("/run/user/1000/omt-a.sock")));
        let b = derived_snapshot_path(Some(std::path::Path::new("/run/user/1000/omt-b.sock")));
        assert!(
            a.is_some() && b.is_some(),
            "no state directory to test with"
        );
        assert_ne!(a, b);
    }

    #[test]
    fn the_same_socket_always_names_the_same_snapshot() {
        // Otherwise a restart is a fresh instance every time, which is the
        // whole thing this file exists to prevent.
        let path = std::path::Path::new("/run/user/1000/omt-502.sock");
        assert_eq!(snapshot_path_for(Some(path)), snapshot_path_for(Some(path)));
    }

    #[test]
    fn a_snapshot_is_named_something_a_person_can_read_in_a_listing() {
        // The socket path is not filename-safe and is long. A directory full
        // of `-var-folders-2x-...-T-omt-502.sock.json` is one nobody can use.
        let path = derived_snapshot_path(Some(std::path::Path::new(
            "/var/folders/2x/hqbz74zs/T/omt-502.sock",
        )))
        .expect("a path");
        let name = path
            .file_name()
            .expect("a name")
            .to_string_lossy()
            .into_owned();
        assert!(name.starts_with("session-"), "{name}");
        assert!(name.ends_with(".json"), "{name}");
        assert!(name.len() < 40, "{name}");
    }

    #[test]
    fn an_explicit_override_wins_over_everything() {
        // What a test or a service manager needs: one variable, no guessing at
        // which of three environment variables the process was started with.
        // SAFETY: single-threaded at this point in the test.
        #[allow(unsafe_code, reason = "set_var is unsafe in edition 2024")]
        unsafe {
            std::env::set_var("OMT_STATE_FILE", "/tmp/omt-explicit.json");
        }
        let path = snapshot_path_for(Some(std::path::Path::new("/ignored.sock")));
        #[allow(unsafe_code, reason = "set_var is unsafe in edition 2024")]
        unsafe {
            std::env::remove_var("OMT_STATE_FILE");
        }
        assert_eq!(
            path,
            Some(std::path::PathBuf::from("/tmp/omt-explicit.json"))
        );
    }
}
