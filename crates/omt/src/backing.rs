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
#[allow(
    dead_code,
    reason = "the attached half of the seam: read the moment the terminal \
              attaches, and kept whole rather than grown a field at a time, \
              because a partial view is what makes a migration stall halfway"
)]
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
#[allow(
    dead_code,
    reason = "the view model the row builders move to next; see this change's \
              tasks, section 7a"
)]
pub(crate) struct Reported {
    /// Which session.
    pub(crate) id: SessionId,
    /// What it is called.
    pub(crate) title: String,
    /// Which agent is in it, short enough for a column.
    pub(crate) agent: Option<String>,
    /// What the agent in it is doing, by the name that state travels under.
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

#[allow(
    dead_code,
    reason = "`write` and `lost` are the attached path's, and the terminal \
              still runs the local one until the surfaces above the seam read \
              a roster rather than an instance"
)]
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
                    // Dropped rather than acted on: a client is not the host,
                    // and a queue nobody reads grows with the session's whole
                    // output.
                    drop(mirror.drain_actions());
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

/// Open a session in an instance and start watching it.
///
/// The attached counterpart of spawning a pty locally: the instance does the
/// spawning, and what comes back is a mirror this terminal draws.
///
/// # Errors
/// Fails if the instance refuses, or answers with something unusable.
pub(crate) fn open_session(
    attached: &mut Attached,
    workspace: &str,
    program: Option<&str>,
    cols: u16,
    rows: u16,
) -> Result<SessionId> {
    let created = attached.client.call(
        "session.create",
        serde_json::json!({
            "workspace": workspace,
            "program": program,
            "cols": cols,
            "rows": rows,
        }),
        true,
    )?;
    let id = created["session"]
        .as_str()
        .and_then(SessionId::from_wire)
        .context("the instance created no session")?;
    watch(attached, id, cols, rows)?;
    Ok(id)
}

/// Take the writer token for a session and start mirroring it.
///
/// Used both for a session this terminal just created and for one it found
/// already running — which is the same thing from here, and is the whole point
/// of a session that outlives its terminal.
///
/// # Errors
/// Fails if the subscription cannot be sent.
pub(crate) fn watch(attached: &mut Attached, id: SessionId, cols: u16, rows: u16) -> Result<()> {
    // The token, like any other actor. Not forced: another terminal or a phone
    // may be holding it, and taking it from them because this process happens
    // to be starting would be exactly the rudeness the token exists to prevent.
    let claim = attached.client.call(
        "session.acquire",
        serde_json::json!({ "session": id.to_wire(), "force": false }),
        true,
    );
    // Somebody else is typing. The session is still watchable — reading over
    // somebody's shoulder is a feature, not a failure — and input is refused
    // at the instance, which is where that decision belongs.
    let epoch = claim.map_or(0, |answer| answer["epoch"].as_u64().unwrap_or(0));
    attached.epochs.insert(id, epoch);

    // One stream per session, numbered in the order they were asked for. The
    // header carries sixteen bits and a session id is far wider, so the
    // mapping is per-connection rather than global.
    let stream = u16::try_from(attached.streams.len()).unwrap_or(u16::MAX);
    attached.streams.insert(id, stream);
    attached.mirrors.insert(
        id,
        omt_term::Terminal::new(omt_term::TermConfig {
            size: omt_term::GridSize::new(cols, rows),
            ..omt_term::TermConfig::default()
        }),
    );
    let watching: Vec<SessionId> = attached.streams.keys().copied().collect();
    attached.client.subscribe(watching)?;
    Ok(())
}

/// What the instance says about its sessions, as this terminal reads them.
///
/// # Errors
/// Fails if the instance refuses or has gone.
pub(crate) fn refresh_roster(attached: &mut Attached) -> Result<()> {
    let listed = attached
        .client
        .call("session.list", serde_json::json!({}), false)?;
    attached.roster = listed["sessions"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    Some(Reported {
                        id: row["id"].as_str().and_then(SessionId::from_wire)?,
                        title: row["title"].as_str().unwrap_or("session").to_owned(),
                        agent: row["agent"].as_str().map(str::to_owned),
                        agent_state: row["agent_state"].as_str().map(str::to_owned),
                        threads: row["threads"].as_u64().unwrap_or(0) as usize,
                        blocked: row["blocked"].as_u64().unwrap_or(0) as usize,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(())
}

/// Connect to an instance, starting one if there is none.
///
/// # Errors
/// Fails when no instance can be reached or started, or when several are
/// running and none was named.
pub(crate) fn join() -> Result<Attached> {
    let socket = crate::lifecycle::instance_socket()?;
    let client = crate::client::Client::connect(&socket)
        .with_context(|| format!("attaching to {}", socket.display()))?;
    Ok(Attached {
        client,
        mirrors: std::collections::BTreeMap::new(),
        streams: std::collections::BTreeMap::new(),
        roster: Vec::new(),
        epochs: std::collections::BTreeMap::new(),
    })
}

/// The session this terminal should show first.
///
/// What was left running in this directory, or a new one when there is
/// nothing. Reusing what is there is the entire point — a terminal that made a
/// fresh session every time would keep the *processes* alive and show nobody
/// their work.
///
/// # Errors
/// Fails if the instance refuses to open the workspace or make a session.
pub(crate) fn first_session(
    attached: &mut Attached,
    cwd: &str,
    cols: u16,
    rows: u16,
) -> Result<SessionId> {
    let workspace =
        attached
            .client
            .call("workspace.open", serde_json::json!({ "root": cwd }), true)?;
    let workspace = workspace["id"]
        .as_str()
        .context("the instance opened no workspace")?
        .to_owned();

    refresh_roster(attached)?;
    // The first one that is still alive. Ordered by the instance, which knows
    // when each was created; picking the newest would mean a terminal that
    // reopened showed the session somebody made last rather than the one they
    // were working in.
    if let Some(existing) = attached.roster.first().map(|s| s.id) {
        watch(attached, existing, cols, rows)?;
        return Ok(existing);
    }
    open_session(attached, &workspace, None, cols, rows)
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
