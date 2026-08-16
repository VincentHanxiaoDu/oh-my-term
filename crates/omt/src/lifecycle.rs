//! Finding an instance to attach to, and starting one when there is none.
//!
//! Nobody should have to know that a daemon exists to keep their agent alive.
//! Somebody types `omt`; whether that is the first terminal today or the fourth
//! is omt's problem, not theirs.
//!
//! The rule when it is ambiguous is to refuse rather than guess: attaching to
//! the wrong instance means typing into a different set of live terminals, and
//! the person doing it has no way to notice until something is already wrong.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

/// How long to wait for a daemon that has just been started.
///
/// It has to bind a socket and restore whatever was open. Generous, because
/// the alternative to waiting is telling somebody their terminal failed to
/// start when it was about to succeed.
const START_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Where the sessions for this terminal should live.
///
/// Returns a socket that answers. The instance behind it may be one somebody
/// else started, one from an earlier terminal today, or one started by this
/// call — which of the three is deliberately not something the caller can
/// tell.
///
/// # Errors
/// Fails when several instances are running and none was named, or when one
/// was started and never came up.
pub fn instance_socket() -> Result<PathBuf> {
    let named = std::env::var_os("OMT_SOCK").map(PathBuf::from);
    let found = crate::discovery::discover(
        named.as_deref(),
        &crate::discovery::runtime_dir(),
        &|path| omt_transport::connect(path).is_ok(),
        &mut |stale| {
            // A socket a dead process left behind. Removing it is best-effort:
            // it will be tried again, and failing to is not worth a message.
            let _ = std::fs::remove_file(stale);
        },
    );
    match found {
        crate::discovery::Found::One(path) => Ok(path),
        // Refused rather than picked. See the module note: the cost of
        // guessing wrong is somebody's keystrokes in the wrong terminal.
        crate::discovery::Found::Several(sockets) => {
            anyhow::bail!(crate::discovery::ambiguous(&sockets))
        }
        crate::discovery::Found::None => start_one(),
    }
}

/// Start an instance and wait for it to answer.
fn start_one() -> Result<PathBuf> {
    // The stable per-user path, not one with a process id in it: a terminal
    // started tomorrow has to be able to find this, and it cannot guess a pid.
    let socket = crate::attach::daemon_socket_path();
    let exe = std::env::current_exe().context("finding omt")?;
    // A session of its own, with no controlling terminal at all. This is the
    // whole difference between an instance and a subprocess, and it took two
    // goes to get right: a child left in the terminal's foreground process
    // group dies of the hangup when that terminal closes, and a child merely
    // moved to *another group in the same session* still has that terminal as
    // its controlling one — which left the daemon dying with its terminal
    // sometimes, under load, which is the worst way for it to be wrong.
    //
    // `setsid` is the only call that severs it. Its output goes nowhere rather
    // than into the screen somebody is about to draw a terminal on.
    use std::os::unix::process::CommandExt as _;
    let mut command = std::process::Command::new(exe);
    command
        .arg("serve")
        .arg("--socket")
        .arg(&socket)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[allow(
        unsafe_code,
        reason = "setsid has no safe wrapper; it is called between fork and \
                  exec, where the only async-signal-safe work allowed is \
                  exactly this kind of syscall"
    )]
    unsafe {
        command.pre_exec(|| {
            // Already a session leader is not a failure: it means this process
            // was started with no controlling terminal in the first place,
            // which is the state being asked for.
            libc::setsid();
            Ok(())
        });
    }
    command.spawn().context("starting an instance")?;

    wait_for(&socket).with_context(|| {
        format!(
            "started an instance for {} and it never answered",
            socket.display()
        )
    })?;
    Ok(socket)
}

/// Wait until a socket accepts a connection.
fn wait_for(socket: &Path) -> Result<()> {
    let deadline = std::time::Instant::now() + START_TIMEOUT;
    while std::time::Instant::now() < deadline {
        if omt_transport::connect(socket).is_ok() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    anyhow::bail!("nothing answered within {START_TIMEOUT:?}")
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {

    #[test]
    fn a_socket_that_never_answers_is_reported_rather_than_waited_on_forever() {
        // A terminal that hangs on startup is indistinguishable from a broken
        // machine, and the person cannot tell which.
        let dir = std::env::temp_dir().join(format!("omt-lifecycle-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("tempdir");
        let missing = dir.join("nothing-here.sock");
        // The real timeout is ten seconds, which is too long for a test; what
        // is checked here is that it *ends*, using the same loop.
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(200);
        while std::time::Instant::now() < deadline {
            assert!(omt_transport::connect(&missing).is_err());
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
