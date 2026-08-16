//! The binary, run as a user runs it.
//!
//! Every other test drives a library. This one execs `omt` on a pty, types into
//! it, and reads what comes back — which is the only way to catch the things
//! that only exist when all of it runs at once: raw mode, the draw loop, and
//! keystrokes making the round trip through the writer token into a real shell.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]

use std::io::{Read, Write};
use std::time::{Duration, Instant};

use omt_pty::{ExitStatus, Pty, PtyConfig, PtySize};

/// Run the built binary on a pty of our own.
fn omt_on_a_pty() -> Pty {
    let exe = std::env::current_exe().expect("test exe");
    // target/debug/deps/<test> → target/debug/omt
    let binary = exe
        .parent()
        .and_then(std::path::Path::parent)
        .expect("target dir")
        .join("omt");
    assert!(binary.exists(), "{} was not built", binary.display());

    Pty::spawn(&PtyConfig {
        program: binary,
        args: Vec::new(),
        size: PtySize::new(80, 24),
        env: vec![
            // A predictable shell, so the test does not depend on whose machine
            // it runs on.
            ("SHELL".to_owned(), "/bin/sh".to_owned()),
            ("PS1".to_owned(), "$ ".to_owned()),
            // Its own sessions. Attached is the default, and a test that
            // attached would put sessions into whichever instance happens to
            // be running on the machine it is run on — somebody's real one,
            // with their work in it.
            ("OMT_LOCAL".to_owned(), "1".to_owned()),
        ],
        ..PtyConfig::default()
    })
    .expect("spawn omt")
}

fn read_until(pty: &Pty, needle: &str, within: Duration) -> String {
    // Non-blocking, or the deadline is never reached: an idle omt writes
    // nothing, so a blocking read waits forever between checks.
    pty.set_nonblocking(true).expect("nonblocking");
    let mut reader = pty.reader();
    let mut out = String::new();
    let mut buf = [0u8; 8192];
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                out.push_str(&String::from_utf8_lossy(&buf[..n]));
                if out.contains(needle) {
                    break;
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::Interrupted | std::io::ErrorKind::WouldBlock
                ) =>
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(_) => break,
        }
    }
    out
}

/// Both assertions live in one test on purpose.
///
/// The pty half drives a real interactive program, so it is sensitive to when
/// things happen. Sharing a binary with a test that runs concurrently made it
/// pass alone and fail in a full run — the worst kind of flake, because it
/// looks like the code and is the harness.
#[test]
fn the_binary_runs_a_shell_and_answers_version() {
    version_does_not_need_a_terminal();
    runs_a_shell_and_types_into_it();
}

fn runs_a_shell_and_types_into_it() {
    let mut pty = omt_on_a_pty();

    // Wait for evidence rather than a fixed sleep: omt has to reach raw mode
    // and draw once before a keystroke means anything, and how long that takes
    // depends on the machine.
    let start = read_until(&pty, "\u{1b}[", Duration::from_secs(10));
    assert!(
        start.contains('\u{1b}'),
        "omt never drew anything:\n{start:?}"
    );
    pty.writer()
        .write_all(b"echo it-really-runs\r")
        .expect("type");

    let seen = read_until(&pty, "it-really-runs", Duration::from_secs(15));
    assert!(
        seen.contains("it-really-runs"),
        "the keystroke never reached a shell, or its output never came back:\n{seen}"
    );

    // Ctrl-B d quits. The prefix exists because every bare key belongs to the
    // program underneath, and `Ctrl-B` is tmux's — the byte is written here
    // rather than derived, so changing the default leader fails this test
    // instead of silently shipping a key nobody is told about.
    //
    // This one owns its sessions — see the spawn above — so leaving does end
    // the shell in it, and omt asks first (05 §4.1b). Enter is the answer, in
    // the list itself.
    pty.writer().write_all(b"\x02d").expect("quit");
    let asked = read_until(&pty, "ends when omt does", Duration::from_secs(10));
    assert!(
        asked.contains("ends when omt does"),
        "quitting took a running shell with no warning:\n{asked}"
    );
    pty.writer().write_all(b"\r").expect("confirm");

    // Keep draining while waiting. Leaving raw mode writes escape sequences,
    // and a reader that stopped reading would fill the pty's buffer and block
    // the very exit it is waiting for.
    let mut reader = pty.reader();
    let mut sink = [0u8; 4096];
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        let _ = reader.read(&mut sink);
        if let Some(status) = pty.try_wait().expect("wait") {
            assert_eq!(
                status,
                ExitStatus::Code(0),
                "quitting should be a clean exit"
            );
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("omt did not exit after the quit chord was confirmed");
}

fn version_does_not_need_a_terminal() {
    // A version check runs in scripts, pipes and CI. If it entered raw mode it
    // would fail everywhere that matters.
    let out = std::process::Command::new(
        std::env::current_exe()
            .expect("test exe")
            .parent()
            .and_then(std::path::Path::parent)
            .expect("target dir")
            .join("omt"),
    )
    .arg("--version")
    .output()
    .expect("run");
    assert!(out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stdout).starts_with("omt "),
        "{:?}",
        out.stdout
    );
}
