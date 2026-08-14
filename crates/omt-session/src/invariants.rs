//! What must be true of an instance after every operation.
//!
//! `docs/architecture/05-session-model.md` §11. These are not documentation:
//! they are checked, and the property tests call them after every operation so
//! that a bug is caught at the operation that caused it rather than three
//! operations later where the symptom is unrecognisable.
//!
//! Each violation names the objects involved. "Invariant 2 failed" sends
//! somebody reading a list; "pane p_3 in view v_1 points at session s_9, which
//! is not in the instance" sends them to the line.
//!
//! Not every invariant in §11 is checkable from here. Six and ten are about a
//! session's sequence number and its negotiated pty size, which live above this
//! crate — those are named in [`UNCHECKED`] rather than quietly skipped, so the
//! list of what this file does *not* guarantee is as visible as what it does.

use crate::tree::{Instance, SessionState};

/// A broken invariant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// Which invariant, numbered as in §11.
    pub number: u8,
    /// What is wrong, naming the objects.
    pub detail: String,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invariant {}: {}", self.number, self.detail)
    }
}

/// Invariants §11 states that this crate cannot see.
///
/// Named rather than skipped: a checker that silently covers eight of ten reads
/// as a guarantee of ten.
pub const UNCHECKED: &[(u8, &str)] = &[
    (
        6,
        "a session's seq is strictly monotonic across restore — the sequence lives in the daemon, not the tree",
    ),
    (
        10,
        "a pty session's size matches what its SizePolicy resolves to — the negotiation is in the daemon and the transport",
    ),
];

/// Check every invariant this crate can see.
///
/// Returns every violation rather than the first: a tree that has gone wrong
/// has usually gone wrong in more than one way, and fixing them one build at a
/// time is slow.
#[must_use]
pub fn check(instance: &Instance) -> Vec<Violation> {
    let mut out = Vec::new();
    let sessions: std::collections::BTreeSet<_> =
        instance.sessions().iter().map(|s| s.id).collect();

    for workspace in instance.workspaces() {
        // 1. Every pane is in exactly one view, and its id is unique there.
        let mut seen_panes = std::collections::BTreeMap::new();
        for view in workspace.views.values() {
            for pane in &view.panes {
                if let Some(other) = seen_panes.insert(pane.id, view.id) {
                    out.push(Violation {
                        number: 1,
                        detail: format!(
                            "pane {} is in view {} and also in view {}",
                            pane.id, other, view.id
                        ),
                    });
                }

                // 2. Every pane names a live session.
                if !sessions.contains(&pane.session) {
                    out.push(Violation {
                        number: 2,
                        detail: format!(
                            "pane {} in view {} points at session {}, which is not in the instance",
                            pane.id, view.id, pane.session
                        ),
                    });
                }
            }

            // A focus or zoom naming a pane that is not there leaves a view
            // that renders nothing and cannot be recovered from by the UI.
            if let Some(focus) = view.focus
                && !view.panes.iter().any(|p| p.id == focus)
            {
                out.push(Violation {
                    number: 1,
                    detail: format!(
                        "view {} focuses pane {focus}, which it does not hold",
                        view.id
                    ),
                });
            }
            if let Some(zoom) = view.zoom
                && !view.panes.iter().any(|p| p.id == zoom)
            {
                out.push(Violation {
                    number: 1,
                    detail: format!("view {} zooms pane {zoom}, which it does not hold", view.id),
                });
            }
        }

        // The primary view must exist, or a client asking for nothing gets
        // nothing.
        if !workspace.views.contains_key(&workspace.primary) {
            out.push(Violation {
                number: 1,
                detail: format!(
                    "workspace {} names primary view {}, which it does not hold",
                    workspace.id, workspace.primary
                ),
            });
        }

        // 8. The workspace's session list and each session's workspace agree.
        for id in &workspace.sessions {
            match instance.session(*id) {
                None => out.push(Violation {
                    number: 8,
                    detail: format!(
                        "workspace {} lists session {id}, which is not in the instance",
                        workspace.id
                    ),
                }),
                Some(session) if session.workspace != workspace.id => out.push(Violation {
                    number: 8,
                    detail: format!(
                        "workspace {} lists session {id}, which belongs to {}",
                        workspace.id, session.workspace
                    ),
                }),
                Some(_) => {}
            }
        }
    }

    for session in instance.sessions() {
        // 8, the other direction.
        let listed = instance
            .workspace(session.workspace)
            .is_some_and(|w| w.sessions.contains(&session.id));
        if !listed {
            out.push(Violation {
                number: 8,
                detail: format!(
                    "session {} says it is in workspace {}, which does not list it",
                    session.id, session.workspace
                ),
            });
        }

        // 7. A session with no process has no writer.
        //
        // A held token on a dead session is worse than a useless one: the UI
        // shows somebody as the writer, and the next person to try is told the
        // seat is taken by a session that cannot receive anything anyway.
        let dead = matches!(
            session.state,
            SessionState::Exited { .. } | SessionState::Orphaned
        );
        if dead && !session.writer.is_free() {
            out.push(Violation {
                number: 7,
                detail: format!(
                    "session {} is {:?} and still holds a writer token",
                    session.id, session.state
                ),
            });
        }

        // 9. No viewer is listed twice. A duplicate makes a presence count
        // report two people where there is one.
        let mut seen = std::collections::BTreeSet::new();
        for viewer in &session.viewers {
            if !seen.insert(*viewer) {
                out.push(Violation {
                    number: 9,
                    detail: format!("session {} lists viewer {viewer} twice", session.id),
                });
            }
        }
    }

    out
}

/// Panic if anything is broken.
///
/// For use after an operation in a test or a debug build, where the useful
/// behaviour is to stop at the operation that caused it.
///
/// # Panics
/// If any invariant is violated.
pub fn assert_ok(instance: &Instance) {
    let violations = check(instance);
    assert!(
        violations.is_empty(),
        "{}",
        violations
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;
    use crate::tree::{SessionKind, SessionMode};

    fn instance_with_a_session() -> (Instance, omt_types::SessionId) {
        let mut instance = Instance::default();
        let workspace = instance.open_workspace("/tmp/project").expect("workspace");
        let session = instance
            .create_session(workspace, SessionKind::Shell, SessionMode::Pty)
            .expect("session");
        (instance, session)
    }

    #[test]
    fn a_fresh_instance_is_consistent() {
        assert_ok(&Instance::default());
    }

    #[test]
    fn an_instance_with_a_session_is_consistent() {
        let (instance, _) = instance_with_a_session();
        assert_ok(&instance);
    }

    #[test]
    fn every_ordinary_operation_leaves_it_consistent() {
        // The reason this exists: a bug is caught at the operation that caused
        // it rather than three operations later.
        let mut instance = Instance::default();
        let workspace = instance.open_workspace("/tmp/a").expect("workspace");
        assert_ok(&instance);

        let first = instance
            .create_session(workspace, SessionKind::Shell, SessionMode::Pty)
            .expect("first");
        assert_ok(&instance);

        let second = instance
            .create_session(workspace, SessionKind::Shell, SessionMode::Pty)
            .expect("second");
        assert_ok(&instance);

        instance.close_session(first).expect("closed");
        assert_ok(&instance);

        instance.close_session(second).expect("closed");
        assert_ok(&instance);
    }

    #[test]
    fn a_closed_session_leaves_no_pane_pointing_at_it() {
        // Invariant 2. A pane pointing at a session that is gone renders
        // nothing and cannot be recovered from by the UI.
        let (mut instance, session) = instance_with_a_session();
        instance.close_session(session).expect("closed");
        let violations = check(&instance);
        assert!(
            violations.is_empty(),
            "closing a session left a dangling pane: {violations:?}"
        );
    }

    #[test]
    fn the_two_directions_of_membership_are_both_checked() {
        // Invariant 8 is a mutual agreement, and checking one direction only
        // would miss exactly the case where one side was updated.
        let (mut instance, session) = instance_with_a_session();
        instance.forget_session_for_test(session);
        let violations = check(&instance);
        assert!(
            violations.iter().any(|v| v.number == 8),
            "a workspace listing a session that is gone was not caught: {violations:?}"
        );
    }

    #[test]
    fn a_violation_names_the_objects_rather_than_the_rule_number() {
        // "Invariant 8 failed" sends somebody reading a list; the ids send them
        // to the line.
        let (mut instance, session) = instance_with_a_session();
        instance.forget_session_for_test(session);
        let violation = check(&instance)
            .into_iter()
            .find(|v| v.number == 8)
            .expect("caught");
        assert!(
            violation.detail.contains(&session.to_string()),
            "{violation}"
        );
    }

    #[test]
    fn every_violation_is_reported_rather_than_only_the_first() {
        // A tree that has gone wrong has usually gone wrong in more than one
        // way, and fixing them one build at a time is slow.
        let mut instance = Instance::default();
        let workspace = instance.open_workspace("/tmp/a").expect("workspace");
        let a = instance
            .create_session(workspace, SessionKind::Shell, SessionMode::Pty)
            .expect("a");
        let b = instance
            .create_session(workspace, SessionKind::Shell, SessionMode::Pty)
            .expect("b");
        instance.forget_session_for_test(a);
        instance.forget_session_for_test(b);
        assert!(check(&instance).len() >= 2);
    }

    #[test]
    fn what_this_crate_cannot_check_is_named() {
        // A checker that silently covers eight of ten reads as a guarantee of
        // ten.
        let numbers: Vec<u8> = UNCHECKED.iter().map(|(n, _)| *n).collect();
        assert_eq!(numbers, vec![6, 10]);
        for (_, why) in UNCHECKED {
            assert!(why.len() > 30, "{why}");
        }
    }
}
