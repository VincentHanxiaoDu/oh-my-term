//! Attaching a terminal to a daemon that already has the sessions.
//!
//! This is what makes a session outlive the thing looking at it. The sessions
//! live in `omt serve`; a terminal attaches, draws what it is told and sends
//! keys back, and when it goes away nothing on the far side notices.
//!
//! Everything here goes through the ordinary capability calls — `session.list`,
//! `session.snapshot`, `session.write`. There is deliberately no faster local
//! path: a second way to reach a session is a second set of rules to keep in
//! step, and the whole architecture rests on there being one.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use omt_catalog::RequestId;
use omt_proto::{Call, CallOutcome, ProtoMessage};
use omt_types::DeviceId;

/// Where a user's daemon listens.
///
/// One per user, without the process id in it. The id was in the old path and
/// that is exactly why nothing could ever find a running daemon: a socket only
/// its own process can name is a socket nobody can attach to.
#[must_use]
pub fn daemon_socket_path() -> PathBuf {
    let base = std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    // The uid, so two people on one machine do not collide on a path — and so
    // one cannot connect to the other's by guessing it.
    #[allow(
        unsafe_code,
        reason = "getuid has no safe wrapper in std and cannot fail"
    )]
    let uid = unsafe { libc::getuid() };
    base.join(format!("omt-{uid}.sock"))
}

/// Whether a daemon is listening there.
///
/// Tried rather than assumed from the file existing: a socket left behind by a
/// process that died is a file that exists and refuses every connection, and
/// "the daemon is running" must mean it answered.
#[must_use]
pub fn daemon_is_running(path: &Path) -> bool {
    omt_transport::connect(path).is_ok()
}

/// A connection to a daemon, speaking the ordinary protocol.
pub struct Attached {
    stream: std::os::unix::net::UnixStream,
    device: DeviceId,
    next: u64,
}

impl Attached {
    /// Connect and shake hands.
    ///
    /// # Errors
    /// Fails if nothing is listening, or if the daemon refuses the handshake.
    pub fn connect(path: &Path) -> Result<Self> {
        let stream = omt_transport::connect(path)
            .with_context(|| format!("connecting to {}", path.display()))?;
        let mut me = Self {
            stream,
            device: DeviceId::new(),
            next: 0,
        };
        me.send(&ProtoMessage::Hello(omt_proto::Hello {
            proto: omt_proto::PROTO_VERSION,
            client: "omt-attach".to_owned(),
            token: None,
        }))?;
        match me.recv()? {
            ProtoMessage::Welcome(_) => Ok(me),
            other => bail!("the daemon answered a hello with {other:?}"),
        }
    }

    /// Invoke a capability and hand back what it produced.
    ///
    /// # Errors
    /// Fails if the connection breaks or the capability refused.
    pub fn call(
        &mut self,
        capability: &str,
        input: serde_json::Value,
        command: bool,
    ) -> Result<serde_json::Value> {
        self.next += 1;
        let request = RequestId {
            device: self.device,
            n: self.next,
        };
        self.send(&ProtoMessage::Call(Call {
            request,
            capability: capability.to_owned(),
            input,
            // Minted per call at intent time. The daemon refuses a command
            // without one, which is what makes a retry recognisable rather
            // than a second execution.
            intent: command.then(omt_types::IntentId::new),
        }))?;
        loop {
            match self.recv()? {
                ProtoMessage::Result(result) if result.request == request => {
                    return match result.outcome {
                        CallOutcome::Ok { output } => Ok(output),
                        CallOutcome::Err { error } => bail!("{capability}: {}", error.message),
                    };
                }
                // Anything else on the wire belongs to the event stream, which
                // this loop does not consume — dropping it here rather than
                // treating it as an answer is what keeps the two apart.
                _ => {}
            }
        }
    }

    fn send(&mut self, message: &ProtoMessage) -> Result<()> {
        let bytes = serde_json::to_vec(message).context("encoding")?;
        omt_transport::write_frame(&mut self.stream, omt_proto::FrameKind::Text, &bytes)
            .context("writing")?;
        self.stream.flush().ok();
        Ok(())
    }

    fn recv(&mut self) -> Result<ProtoMessage> {
        let (_, payload) =
            omt_transport::read_frame(&mut self.stream).context("the daemon hung up")?;
        serde_json::from_slice(&payload).context("decoding")
    }
}

/// Attach a terminal to a daemon's session.
///
/// # Errors
/// Fails if the daemon cannot be reached, or if the terminal cannot be put into
/// raw mode.
pub fn run(socket: Option<&Path>, program: Option<String>) -> Result<()> {
    let path = socket.map_or_else(daemon_socket_path, Path::to_path_buf);
    let mut daemon = Attached::connect(&path).with_context(|| {
        format!(
            "no omt daemon at {} — start one with `omt serve`",
            path.display()
        )
    })?;

    let (cols, rows) = match crossterm::terminal::size() {
        Ok((c, r)) if c > 0 && r > 1 => (c, r),
        _ => (80, 24),
    };
    let usable_rows = rows.saturating_sub(1).max(1);

    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".to_owned());
    let workspace = daemon.call("workspace.open", serde_json::json!({ "root": cwd }), true)?;
    let workspace_id = workspace["id"].clone();

    // The session already there, if there is one. Attaching to a fresh session
    // every time would defeat the entire point: what survives a restart is only
    // useful if the next attach finds it.
    let listed = daemon.call("session.list", serde_json::json!({}), false)?;
    let existing = listed["sessions"]
        .as_array()
        .and_then(|s| s.first())
        .and_then(|s| s["id"].as_str())
        .map(str::to_owned);

    let session = match existing {
        Some(id) => id,
        None => {
            let created = daemon.call(
                "session.create",
                serde_json::json!({
                    "workspace": workspace_id,
                    "program": program,
                    "cols": cols,
                    "rows": usable_rows,
                }),
                true,
            )?;
            created["session"]
                .as_str()
                .context("the daemon created no session")?
                .to_owned()
        }
    };

    let claim = daemon.call(
        "session.acquire",
        serde_json::json!({ "session": session, "force": false }),
        true,
    )?;
    let epoch = claim["epoch"].as_u64().unwrap_or(0);

    daemon.call(
        "session.resize",
        serde_json::json!({ "session": session, "cols": cols, "rows": usable_rows }),
        true,
    )?;

    let _guard = omt_tui::RawGuard::enter().context("entering raw mode")?;
    let mut out = io::stdout();
    let mut armed = false;
    let mut last = String::new();

    loop {
        let snapshot = daemon.call(
            "session.snapshot",
            serde_json::json!({ "session": session }),
            false,
        )?;
        let painted = paint(&snapshot);
        if painted != last {
            write!(out, "\x1b[H\x1b[2J{painted}").context("drawing")?;
            out.flush().ok();
            last = painted;
        }

        if crossterm::event::poll(std::time::Duration::from_millis(30))
            .context("polling for input")?
        {
            let event = crossterm::event::read().context("reading input")?;
            let (input, next) = omt_tui::translate(&event, armed, omt_input::EncodeMode::default());
            armed = next;
            match input {
                omt_tui::Input::Bytes(bytes) => {
                    daemon.call(
                        "session.write",
                        serde_json::json!({
                            "session": session,
                            "text": String::from_utf8_lossy(&bytes),
                            "epoch": epoch,
                        }),
                        true,
                    )?;
                    // Redrawn on the next pass rather than here: the daemon has
                    // to have pumped the pty before the screen means anything.
                }
                omt_tui::Input::Resize { cols, rows } => {
                    daemon.call(
                        "session.resize",
                        serde_json::json!({
                            "session": session,
                            "cols": cols,
                            "rows": rows.saturating_sub(1).max(1),
                        }),
                        true,
                    )?;
                    last.clear();
                }
                // Detaching leaves the session running, which is the whole
                // point of it living in the daemon.
                omt_tui::Input::Detach => break,
                _ => {}
            }
        }
    }

    write!(out, "\x1b[H\x1b[2J").ok();
    out.flush().ok();
    Ok(())
}

/// Turn a snapshot into the text to draw.
///
/// Text rather than styled runs, for now: this path exists to prove a session
/// outlives its terminal, and colour on top of that is the same `paint` the web
/// client already has.
fn paint(snapshot: &serde_json::Value) -> String {
    snapshot["rows"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .map(|row| {
                    row.as_array()
                        .map(|runs| {
                            runs.iter()
                                .filter_map(|r| r["text"].as_str())
                                .collect::<String>()
                        })
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>()
                .join("\r\n")
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_daemon_socket_has_no_process_id_in_it() {
        // A socket only its own process can name is a socket nobody can attach
        // to, which is exactly why sessions could not outlive their terminal.
        let path = daemon_socket_path();
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        assert!(
            !name.contains(&std::process::id().to_string()),
            "the daemon socket is per-process: {name}"
        );
    }

    #[test]
    fn a_socket_file_left_behind_by_a_dead_daemon_is_not_a_running_daemon() {
        // It exists and refuses every connection. Treating the file as proof
        // would make every attach fail with a confusing error instead of
        // starting a daemon.
        let dir = std::env::temp_dir().join("omt-attach-test");
        std::fs::create_dir_all(&dir).ok();
        let path = dir.join("stale.sock");
        std::fs::write(&path, b"not a socket").ok();
        assert!(!daemon_is_running(&path));
    }
}
