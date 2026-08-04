//! Attaching to an omt on another machine.

use std::io::{Read, Write};

use anyhow::{Context, Result};
use omt_transport::Remote;

/// Open a session against a remote instance.
///
/// # Errors
/// Fails if ssh cannot be started, or if the remote refuses.
pub fn attach(destination: &str) -> Result<()> {
    let remote = Remote::new(destination);
    let mut child = remote
        .spawn()
        .with_context(|| format!("running ssh to {destination}"))?;

    let mut to_remote = child.stdin.take().context("ssh stdin")?;
    let mut from_remote = child.stdout.take().context("ssh stdout")?;

    // The same framed protocol the local socket carries. A remote instance is
    // not a different kind of thing — it is the same instance further away,
    // which is why nothing below this line knows which it is talking to.
    let hello = omt_proto::ProtoMessage::Hello(omt_proto::Hello {
        proto: omt_proto::PROTO_VERSION,
        client: "omt-ssh".to_owned(),
        token: None,
    });
    let bytes = serde_json::to_vec(&hello).context("encoding hello")?;
    omt_transport::write_frame(&mut to_remote, omt_proto::FrameKind::Text, &bytes)
        .context("writing hello")?;
    to_remote.flush().ok();

    let (_, payload) = omt_transport::read_frame(&mut from_remote)
        .context("the remote closed before answering")?;
    let reply: omt_proto::ProtoMessage =
        serde_json::from_slice(&payload).context("decoding the remote's answer")?;

    match reply {
        omt_proto::ProtoMessage::Welcome(welcome) => {
            println!(
                "attached to {destination}: protocol {}, {} capabilities",
                welcome.proto,
                welcome.capabilities.len()
            );
        }
        omt_proto::ProtoMessage::Goodbye(goodbye) => {
            // Reported with the remote's own words. A generic "connection
            // failed" here would hide a version mismatch the user can fix.
            anyhow::bail!("{destination} refused: {}", goodbye.detail);
        }
        other => anyhow::bail!("{destination} answered with {other:?} instead of a welcome"),
    }

    // Relay until either side stops. What travels is the framed protocol, so
    // the remote's sessions are reachable through exactly the capabilities a
    // local client uses.
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = from_remote
            .read(&mut buf)
            .context("reading from the remote")?;
        if n == 0 {
            break;
        }
        std::io::stdout()
            .write_all(&buf[..n])
            .context("writing to stdout")?;
    }

    let _ = child.wait();
    Ok(())
}
