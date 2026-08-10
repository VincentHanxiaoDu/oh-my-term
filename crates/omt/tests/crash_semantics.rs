//! Whether the last stop was a stop or a fall.
//!
//! `docs/architecture/05-session-model.md` §8.2. The claim is about what
//! happens when a process dies, so it is checked by killing one.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]

use std::path::Path;
use std::process::{Command, Stdio};

fn binary() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_omt"))
}

/// Start a daemon with its own state and runtime directories.
fn start(state: &Path, runtime: &Path) -> std::process::Child {
    let child = Command::new(binary())
        .arg("serve")
        .env("XDG_STATE_HOME", state)
        .env("XDG_RUNTIME_DIR", runtime)
        .env_remove("OMT_SOCK")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn omt serve");

    // Wait for the marker to carry *this* daemon's pid. Waiting for the file
    // to exist would return immediately when a previous run crashed and left
    // one behind, which is precisely the case these tests set up.
    let marker = state.join("omt").join("running.marker");
    let wanted = child.id().to_string();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if std::fs::read_to_string(&marker).is_ok_and(|pid| pid == wanted) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the daemon never marked itself running"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    child
}

fn marker_of(state: &Path) -> std::path::PathBuf {
    state.join("omt").join("running.marker")
}

#[test]
fn a_running_daemon_is_marked_as_running() {
    let state = tempfile::tempdir().expect("state");
    let runtime = tempfile::tempdir().expect("runtime");
    let mut child = start(state.path(), runtime.path());
    assert!(marker_of(state.path()).exists());
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn an_orderly_stop_clears_the_marker() {
    // Without this every restart reports a crash, and a crash report that is
    // always wrong is worse than none — it is the banner people learn to
    // ignore, and then they ignore the one that mattered.
    let state = tempfile::tempdir().expect("state");
    let runtime = tempfile::tempdir().expect("runtime");
    let child = start(state.path(), runtime.path());

    #[allow(unsafe_code, reason = "sending a signal has no safe wrapper in std")]
    unsafe {
        libc::kill(
            libc::pid_t::try_from(child.id()).expect("a pid"),
            libc::SIGTERM,
        );
    }
    let mut child = child;
    let _ = child.wait();

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while marker_of(state.path()).exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "SIGTERM did not clear the running marker"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[test]
fn a_killed_daemon_leaves_the_marker_behind() {
    // The other half: SIGKILL cannot be handled, which is exactly the case the
    // marker exists to detect. A crash detector that also cleared itself on a
    // crash would detect nothing.
    let state = tempfile::tempdir().expect("state");
    let runtime = tempfile::tempdir().expect("runtime");
    let mut child = start(state.path(), runtime.path());

    child.kill().expect("kill");
    let _ = child.wait();
    std::thread::sleep(std::time::Duration::from_millis(200));

    assert!(
        marker_of(state.path()).exists(),
        "the marker is what tells the next start that omt fell over"
    );
    assert!(
        !omt::shutdown::ended_cleanly(&state.path().join("omt")),
        "and it is read as an unclean end"
    );
}

#[test]
fn the_next_start_after_a_crash_is_told_and_the_one_after_that_is_not() {
    // The marker is rewritten on every start, so a crash is reported once
    // rather than for ever after.
    let state = tempfile::tempdir().expect("state");
    let runtime = tempfile::tempdir().expect("runtime");

    let mut first = start(state.path(), runtime.path());
    first.kill().expect("kill");
    let _ = first.wait();
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(!omt::shutdown::ended_cleanly(&state.path().join("omt")));

    // The next daemon starts, sees the marker, and rewrites it for itself.
    let second = start(state.path(), runtime.path());
    #[allow(unsafe_code, reason = "sending a signal has no safe wrapper in std")]
    unsafe {
        libc::kill(
            libc::pid_t::try_from(second.id()).expect("a pid"),
            libc::SIGTERM,
        );
    }
    let mut second = second;
    let _ = second.wait();
    std::thread::sleep(std::time::Duration::from_millis(300));

    assert!(
        omt::shutdown::ended_cleanly(&state.path().join("omt")),
        "the orderly stop after a crash leaves a clean state"
    );
}
