//! Where the terminal's sessions actually live.
//!
//! One seam, deliberately narrow. Above it are the layout tree, the renderer,
//! the panel, the overlays, copy mode and the status bar — none of which have
//! any business knowing whether the pty they are drawing is in this process or
//! in an instance this process is only watching. Below it are two answers to
//! that question.
//!
//! The narrowness is the test. Every method here is something the frame loop
//! genuinely cannot do for itself; if a surface ever needs a thirteenth, the
//! seam is in the wrong place.
//!
//! **Local** is the terminal owning its own ptys — how omt has always run, and
//! what dies when the process does. **Attached** is a client of an instance
//! that owns them, which is how a session outlives the terminal that started
//! it. They coexist on purpose: a half-migrated terminal is worse than either
//! end of the migration.

use anyhow::{Context as _, Result};
use omt_daemon::Instance;
use omt_types::SessionId;

/// What a frame loop needs of the thing holding its sessions.
///
/// Borrowed rather than owned, and rebuilt per frame. The local instance lives
/// behind the lock the socket server shares, so a `Backing` that owned it would
/// be a second copy of the thing every other surface reads — and the attached
/// one has to outlive a frame, because a connection re-made every eight
/// milliseconds is not a connection.
pub(crate) enum Backing<'a> {
    /// The sessions are in this process.
    Local(&'a mut Instance),
    /// The sessions are somewhere else, and this is watching them.
    Attached(&'a mut Attached),
}

/// A client's view of an instance's sessions.
pub(crate) struct Attached {
    /// The connection.
    pub(crate) client: crate::client::Client,
    /// One emulator per session, fed from the stream.
    ///
    /// The same emulator the local path runs, which is what lets everything
    /// above this file stay the same.
    pub(crate) mirrors: std::collections::BTreeMap<SessionId, omt_term::Terminal>,
    /// Which stream carries which session.
    pub(crate) streams: std::collections::BTreeMap<SessionId, u16>,
    /// What the instance last said about its sessions.
    ///
    /// Refreshed on a timer rather than per frame: it is a socket round trip,
    /// and what it carries — titles, agent state, thread counts — changes on
    /// human timescales.
    pub(crate) roster: Vec<Reported>,
    /// The writer epoch per session, so input is refused rather than landing in
    /// somebody else's command line.
    pub(crate) epochs: std::collections::BTreeMap<SessionId, u64>,
}

/// What the instance says about one session.
#[derive(Debug, Clone)]
pub(crate) struct Reported {
    /// Which session.
    pub(crate) id: SessionId,
    /// What it is called.
    pub(crate) title: String,
    /// What the agent in it is doing, in the shared vocabulary.
    pub(crate) agent_state: Option<String>,
    /// How many agent threads are in it.
    pub(crate) threads: usize,
    /// How many of those are waiting for a human.
    pub(crate) blocked: usize,
}

/// The seam, for one statement.
///
/// Built at each use rather than held: holding it would borrow the instance for
/// as long as the frame, and the frame does a dozen other things with it. The
/// enum is two words, so this costs nothing and keeps every borrow as short as
/// the thing it is for.
pub(crate) fn backing<'a>(
    instance: &'a mut Instance,
    attached: &'a mut Option<Attached>,
) -> Backing<'a> {
    match attached.as_mut() {
        Some(attached) => Backing::Attached(attached),
        None => Backing::Local(instance),
    }
}

impl Backing<'_> {
    /// The emulator for a session, wherever it lives.
    pub(crate) fn terminal(&self, id: SessionId) -> Option<&omt_term::Terminal> {
        match self {
            Self::Local(instance) => instance
                .runtime(id)
                .map(omt_daemon::SessionRuntime::terminal),
            Self::Attached(attached) => attached.mirrors.get(&id),
        }
    }

    /// Move a session forward, and take whatever it produced.
    ///
    /// For a local session that is reading the pty. For an attached one it is
    /// feeding the emulator whatever arrived on the socket — the same shape,
    /// which is why the loop above does not branch.
    pub(crate) fn pump(&mut self, id: SessionId) -> Result<Vec<omt_events::Event>> {
        match self {
            Self::Local(instance) => instance.pump_session(id).context("pumping the session"),
            Self::Attached(attached) => {
                let Some(&stream) = attached.streams.get(&id) else {
                    return Ok(Vec::new());
                };
                let bytes = attached.client.take_output(stream);
                if bytes.is_empty() {
                    return Ok(Vec::new());
                }
                if let Some(mirror) = attached.mirrors.get_mut(&id) {
                    mirror.advance(&bytes);
                    // Drained rather than left to accumulate: the client is not
                    // the host, and a queue nobody reads is a leak that grows
                    // with the session's whole output.
                    mirror.drain_actions();
                }
                // Events come from the instance's own stream, not invented
                // here. What this returns is "something happened", which is
                // what the frame loop uses it for.
                Ok(vec![omt_events::Event {
                    scope: omt_types::SeqScope::Session { session: id },
                    seq: omt_types::Seq::new(0),
                    ts: omt_types::Timestamp::now(),
                    source: omt_events::EventSourceTag::Core,
                    payload: omt_events::EventPayload::Terminal(
                        omt_events::TerminalEvent::Output {
                            bytes: bytes.len() as u64,
                        },
                    ),
                }])
            }
        }
    }

    /// Send keystrokes to a session.
    ///
    /// The local path goes through the writer token like every other actor —
    /// see `LocalPane` — so this one only carries the attached case, where the
    /// token is held by this client and checked at the instance.
    pub(crate) fn write(&mut self, id: SessionId, bytes: &[u8]) -> Result<()> {
        match self {
            // Written by the pane itself, which holds the token and the epoch.
            Self::Local(_) => Ok(()),
            Self::Attached(attached) => {
                let epoch = attached.epochs.get(&id).copied().unwrap_or(0);
                attached
                    .client
                    .call(
                        "session.write",
                        serde_json::json!({
                            "session": id.to_wire(),
                            "text": String::from_utf8_lossy(bytes),
                            "epoch": epoch,
                        }),
                        true,
                    )
                    .map(|_| ())
                    .map_err(anyhow::Error::from)
            }
        }
    }

    /// Tell a session how big its pane is now.
    pub(crate) fn resize(&mut self, id: SessionId, cols: u16, rows: u16) -> Result<()> {
        match self {
            Self::Local(instance) => {
                if let Some(runtime) = instance.runtime_mut(id) {
                    runtime.resize(cols, rows).context("resizing")?;
                }
                Ok(())
            }
            Self::Attached(attached) => {
                if let Some(mirror) = attached.mirrors.get_mut(&id) {
                    // The mirror is resized here and not waited for: the
                    // instance will resize the pty, but the *renderer* is
                    // about to draw into a rect of this size and a mirror that
                    // still believed the old one would be drawn clipped.
                    mirror.resize(omt_term::GridSize::new(cols, rows));
                }
                attached
                    .client
                    .call(
                        "session.resize",
                        serde_json::json!({
                            "session": id.to_wire(),
                            "cols": cols,
                            "rows": rows,
                        }),
                        true,
                    )
                    .map(|_| ())
                    .map_err(anyhow::Error::from)
            }
        }
    }

    /// Whether this is watching an instance that has gone.
    ///
    /// A local terminal cannot lose its sessions without losing itself, so the
    /// question only has an answer on one side.
    pub(crate) fn lost(&self) -> bool {
        match self {
            Self::Local(_) => false,
            Self::Attached(attached) => !attached.client.is_live(),
        }
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

    #[test]
    fn a_local_backing_answers_for_a_session_it_holds() {
        let mut instance = Instance::new();
        let workspace = instance.open_workspace("/w").expect("workspace");
        let session = instance
            .create_session(
                workspace,
                omt_session::SessionKind::Shell,
                omt_session::SessionMode::Pty,
            )
            .expect("session");
        let backing = Backing::Local(&mut instance);
        // No runtime yet, which is a real state: a session record exists before
        // anything is spawned into it.
        assert!(backing.terminal(session).is_none());
        assert!(!backing.lost());
    }
}
