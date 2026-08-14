//! Finding the daemon to talk to.
//!
//! `docs/architecture/22-operations.md` §1.1. Deterministic, never guessed from
//! modification times, and in a fixed order:
//!
//! 1. `$OMT_SOCK` if set. omt injects this into every pane it spawns, so
//!    `omt split` from inside a pane always finds *its* instance rather than
//!    whichever one happens to answer first.
//! 2. Otherwise scan the runtime directory and *connect* to each socket. A
//!    socket that does not accept is a file a dead process left behind; it is
//!    unlinked rather than reported, because the alternative is a directory
//!    that fills with names that mean nothing.
//! 3. Exactly one live instance is used. Several means the user has to say
//!    which, and the list is printed. None means spawn one.
//!
//! The step that is easy to get wrong is the third. Picking the newest socket
//! when there are several would work almost always and silently attach somebody
//! to the wrong instance the rest of the time — and "the wrong instance" here
//! means their commands went to a different set of live terminals.

use std::path::{Path, PathBuf};

/// The directory instance sockets live in.
#[must_use]
pub fn runtime_dir() -> PathBuf {
    std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("omt")
}

/// What discovery concluded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    /// Use this one.
    One(PathBuf),
    /// The user has to choose.
    ///
    /// Listed rather than picked: attaching to the wrong instance sends
    /// commands to a different set of live terminals.
    Several(Vec<PathBuf>),
    /// Nothing is running; spawn.
    None,
}

/// Whether something is listening, tried rather than assumed.
pub type Probe<'a> = &'a dyn Fn(&Path) -> bool;

/// Run the discovery ladder.
///
/// `unlink` is called for each socket that does not answer. Passed in rather
/// than done here so the decision to delete a file is visible at the call site
/// and testable without one.
pub fn discover(
    env_socket: Option<&Path>,
    dir: &Path,
    live: Probe<'_>,
    unlink: &mut dyn FnMut(&Path),
) -> Found {
    if let Some(socket) = env_socket {
        // Honoured even if it does not answer. A pane that was told which
        // instance it belongs to and cannot reach it has a problem to report,
        // not a different instance to silently join.
        return Found::One(socket.to_path_buf());
    }

    let mut sockets: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "sock"))
            .collect(),
        Err(_) => return Found::None,
    };
    // Sorted so the list a user is shown is the same list twice running.
    sockets.sort();

    let mut alive = Vec::new();
    for socket in sockets {
        if live(&socket) {
            alive.push(socket);
        } else {
            unlink(&socket);
        }
    }

    match alive.len() {
        0 => Found::None,
        1 => Found::One(alive.remove(0)),
        _ => Found::Several(alive),
    }
}

/// The message shown when several instances are running.
///
/// Names them, because "several instances are running" without saying which is
/// a dead end.
#[must_use]
pub fn ambiguous(sockets: &[PathBuf]) -> String {
    let mut out = String::from("several instances are running; name one with --instance:\n");
    for socket in sockets {
        out.push_str("  ");
        out.push_str(&socket.display().to_string());
        out.push('\n');
    }
    out
}

/// The socket of a running instance, if there is exactly one.
///
/// A convenience over [`discover`] for the callers that only want to know
/// whether they are about to duplicate something. `Several` answers `None`:
/// with two instances running there is no "the" one, and picking would attach
/// somebody to a different set of live terminals.
#[must_use]
pub fn running_instance() -> Option<PathBuf> {
    let dir = std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    let env_socket = std::env::var("OMT_SOCK").ok().map(PathBuf::from);
    // Nothing is unlinked here. This asks a question; deleting a stale socket
    // is a decision for the code that is about to bind one.
    match discover(
        env_socket.as_deref(),
        &dir,
        &|p| std::os::unix::net::UnixStream::connect(p).is_ok(),
        &mut |_| {},
    ) {
        Found::One(path) => Some(path),
        Found::Several(_) | Found::None => None,
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

    fn dir_with(names: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        for name in names {
            std::fs::write(dir.path().join(name), b"").expect("write");
        }
        dir
    }

    fn nothing_unlinked() -> impl FnMut(&Path) {
        |p: &Path| panic!("unlinked {}", p.display())
    }

    #[test]
    fn the_environment_wins_over_everything_else() {
        // omt injects it into every pane it spawns, so `omt split` finds its
        // own instance rather than whichever answers first.
        let dir = dir_with(&["a.sock", "b.sock"]);
        let found = discover(
            Some(Path::new("/run/mine.sock")),
            dir.path(),
            &|_| true,
            &mut nothing_unlinked(),
        );
        assert_eq!(found, Found::One(PathBuf::from("/run/mine.sock")));
    }

    #[test]
    fn a_named_socket_that_is_dead_is_still_the_answer() {
        // A pane told which instance it belongs to and unable to reach it has a
        // problem to report, not a different instance to silently join.
        let dir = dir_with(&["other.sock"]);
        let found = discover(
            Some(Path::new("/run/mine.sock")),
            dir.path(),
            &|_| false,
            &mut nothing_unlinked(),
        );
        assert_eq!(found, Found::One(PathBuf::from("/run/mine.sock")));
    }

    #[test]
    fn exactly_one_live_socket_is_used() {
        let dir = dir_with(&["only.sock"]);
        let found = discover(None, dir.path(), &|_| true, &mut nothing_unlinked());
        assert_eq!(found, Found::One(dir.path().join("only.sock")));
    }

    #[test]
    fn a_socket_that_does_not_answer_is_unlinked_not_reported() {
        // A file a dead process left behind. The alternative is a directory
        // that fills with names that mean nothing.
        let dir = dir_with(&["stale.sock"]);
        let mut removed = Vec::new();
        let found = discover(None, dir.path(), &|_| false, &mut |p| {
            removed.push(p.to_path_buf());
        });
        assert_eq!(found, Found::None);
        assert_eq!(removed, vec![dir.path().join("stale.sock")]);
    }

    #[test]
    fn several_live_instances_are_listed_rather_than_chosen_between() {
        // Picking the newest would work almost always and silently attach
        // somebody to the wrong set of live terminals the rest of the time.
        let dir = dir_with(&["a.sock", "b.sock"]);
        let found = discover(None, dir.path(), &|_| true, &mut nothing_unlinked());
        let Found::Several(sockets) = found else {
            panic!("several");
        };
        assert_eq!(sockets.len(), 2);
    }

    #[test]
    fn the_list_is_stable_between_runs() {
        // A list that reorders is one a user cannot read twice.
        let dir = dir_with(&["b.sock", "a.sock", "c.sock"]);
        let first = discover(None, dir.path(), &|_| true, &mut nothing_unlinked());
        let second = discover(None, dir.path(), &|_| true, &mut nothing_unlinked());
        assert_eq!(first, second);
    }

    #[test]
    fn stale_sockets_do_not_make_a_single_live_one_ambiguous() {
        // The case this whole ladder exists for: a machine that has crashed a
        // few times has a directory full of dead sockets and one good one.
        let dir = dir_with(&["dead1.sock", "live.sock", "dead2.sock"]);
        let live = dir.path().join("live.sock");
        let found = discover(None, dir.path(), &|p| p == live, &mut |_| {});
        assert_eq!(found, Found::One(live));
    }

    #[test]
    fn files_that_are_not_sockets_are_ignored() {
        // The runtime directory also holds pid files.
        let dir = dir_with(&["pid.txt", "one.sock"]);
        let found = discover(None, dir.path(), &|_| true, &mut nothing_unlinked());
        assert_eq!(found, Found::One(dir.path().join("one.sock")));
    }

    #[test]
    fn a_missing_runtime_directory_means_spawn_rather_than_fail() {
        // A first run on a fresh machine. `omt: server not running` is a design
        // failure, not a message to write well.
        let found = discover(
            None,
            Path::new("/nonexistent/omt"),
            &|_| true,
            &mut nothing_unlinked(),
        );
        assert_eq!(found, Found::None);
    }

    #[test]
    fn the_ambiguous_message_names_the_instances() {
        // "several instances are running" without saying which is a dead end.
        let message = ambiguous(&[PathBuf::from("/run/omt/a.sock")]);
        assert!(message.contains("a.sock"), "{message}");
        assert!(message.contains("--instance"), "{message}");
    }
}
