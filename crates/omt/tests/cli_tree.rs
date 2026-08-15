//! The CLI, against a running instance.
//!
//! `docs/architecture/03-capability-catalog.md` §1 lists the subcommand tree as
//! one of the seven artifacts derived from the declaration, and
//! `22-operations.md` §8.1 says what it has to promise a script. Both are
//! claims about a binary, so they are checked by running one.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_omt"))
}

/// A daemon on a socket of this test's own, so nothing else on the machine can
/// answer and nothing this test does can reach a real instance.
struct Daemon {
    dir: tempfile::TempDir,
}

impl Daemon {
    fn start() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket = dir.path().join("omt.sock");
        let path = socket.clone();
        std::thread::spawn(move || {
            let _ = omt::serve::serve(&path, omt::state::State::default());
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while omt_transport::connect(&socket).is_err() {
            assert!(
                std::time::Instant::now() < deadline,
                "the daemon never started listening"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        Self { dir }
    }

    fn socket(&self) -> PathBuf {
        self.dir.path().join("omt.sock")
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        Command::new(binary())
            .args(args)
            .env("OMT_SOCK", self.socket())
            // And everything else that says where omt keeps things. A test
            // that pins the socket and lets the snapshot, the rules and the
            // blobs come from the machine has pinned the one it thought of.
            .env_remove("OMT_STATE_FILE")
            .env_remove("OMT_AGENT_RULES")
            .env_remove("OMT_MEDIA_DIR")
            .output()
            .expect("run omt")
    }
}

fn run_without_a_daemon(args: &[&str], runtime: &Path) -> std::process::Output {
    Command::new(binary())
        .args(args)
        .env_remove("OMT_SOCK")
        .env_remove("OMT_STATE_FILE")
        .env_remove("OMT_AGENT_RULES")
        .env_remove("OMT_MEDIA_DIR")
        // Point discovery at a directory with nothing in it, so this cannot
        // find a daemon the developer happens to be running.
        .env("XDG_RUNTIME_DIR", runtime)
        .output()
        .expect("run omt")
}

#[test]
fn a_capability_is_reachable_as_a_subcommand_without_being_written_out() {
    // The point of deriving the tree: `session.list` is declared once and is a
    // command line without anybody adding a match arm for it.
    let daemon = Daemon::start();
    let out = daemon.run(&["session", "list"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let parsed: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("stdout is the capability's output");
    assert!(parsed.get("sessions").is_some(), "{parsed}");
}

#[test]
fn stdout_is_data_and_stderr_is_everything_else() {
    // So `omt session list --json | jq` is always safe, whatever omt has to say
    // about it.
    let daemon = Daemon::start();
    let out = daemon.run(&["session", "list", "--json"]);
    serde_json::from_slice::<serde_json::Value>(&out.stdout)
        .expect("stdout parses as JSON on its own");
}

#[test]
fn a_capability_that_does_not_exist_fails_rather_than_hanging() {
    let daemon = Daemon::start();
    let out = daemon.run(&["nosuch", "verb"]);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty(), "nothing went to the data channel");
}

#[test]
fn a_usage_error_exits_two() {
    // A script retrying on 2 would retry forever; one retrying on 1 might
    // succeed, so the two must not be the same code.
    let daemon = Daemon::start();
    let out = daemon.run(&["session", "capture", "--session"]);
    assert_eq!(
        out.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn a_line_that_names_no_capability_exits_two_and_says_what_one_looks_like() {
    let daemon = Daemon::start();
    let out = daemon.run(&["session"]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("omt session list"), "{stderr}");
}

#[test]
fn an_unreachable_instance_exits_five() {
    // Distinct from a failure: "I could not ask" and "it said no" have opposite
    // retry answers, which is the whole reason §8.1 lists seven codes.
    let empty = tempfile::tempdir().expect("tempdir");
    let out = run_without_a_daemon(&["session", "list"], empty.path());
    assert_eq!(
        out.status.code(),
        Some(5),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn a_refusal_from_the_instance_exits_one_not_five() {
    // The instance was reached and answered; that is a different situation from
    // not reaching it.
    let daemon = Daemon::start();
    let out = daemon.run(&[
        "session",
        "capture",
        "--session",
        "sess_00000000000000000000000000000000",
    ]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn arguments_reach_the_capability_with_their_types_intact() {
    // A schema wanting an integer will not take `"40"`, so the command line has
    // to mean 40.
    let daemon = Daemon::start();
    let created = daemon.run(&["workspace", "list"]);
    assert!(created.status.success());

    let out = daemon.run(&["task", "list", "--include-finished", "true"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json");
    assert!(parsed.get("tasks").is_some(), "{parsed}");
}

#[test]
fn a_command_is_dispatched_as_a_command_and_a_query_as_a_query() {
    // A command needs an intent id and a query must not carry one. Getting it
    // backwards refuses every mutation, so this runs one of each.
    let daemon = Daemon::start();
    let query = daemon.run(&["instance", "info"]);
    assert!(
        query.status.success(),
        "{}",
        String::from_utf8_lossy(&query.stderr)
    );

    // Opening one is itself a command, so this exercises the mutation path
    // twice over.
    let opened = daemon.run(&["workspace", "open", "--root", "/tmp"]);
    assert!(
        opened.status.success(),
        "{}",
        String::from_utf8_lossy(&opened.stderr)
    );
    let parsed: serde_json::Value = serde_json::from_slice(&opened.stdout).expect("json");
    let workspace = parsed["workspace"]
        .as_str()
        .or_else(|| parsed["id"].as_str())
        .expect("the opened workspace's id")
        .to_owned();
    let command = daemon.run(&["session", "create", "--workspace", &workspace]);
    assert!(
        command.status.success(),
        "{}",
        String::from_utf8_lossy(&command.stderr)
    );
}
