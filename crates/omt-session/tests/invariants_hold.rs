//! The invariants, after every operation, over sequences nobody wrote by hand.
//!
//! `docs/architecture/05-session-model.md` §11 says the property tests call
//! `check_invariants` after every operation. A checker nothing drives finds
//! nothing, so this is the driver: pseudo-random operation sequences, with the
//! whole instance checked after each step.
//!
//! The generator is a small linear congruential one rather than a fuzzing
//! dependency, and it is seeded deterministically. A property test that fails
//! only sometimes, and cannot be re-run to see it fail again, is a test people
//! delete.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]

use omt_session::invariants::{assert_ok, check};
use omt_session::tree::{Instance, SessionKind, SessionMode};

/// Deterministic, so a failure can be reproduced from its seed alone.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }
}

#[test]
fn every_operation_sequence_leaves_the_instance_consistent() {
    for seed in 0..64u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let mut instance = Instance::default();
        let mut workspaces = Vec::new();
        let mut sessions = Vec::new();

        for step in 0..200 {
            match rng.below(5) {
                0 => {
                    let root = format!("/tmp/ws-{}", rng.below(4));
                    if let Ok(id) = instance.open_workspace(&root)
                        && !workspaces.contains(&id)
                    {
                        workspaces.push(id);
                    }
                }
                1 if !workspaces.is_empty() => {
                    let ws = workspaces[rng.below(workspaces.len() as u64) as usize];
                    let mode = if rng.below(2) == 0 {
                        SessionMode::Pty
                    } else {
                        SessionMode::Native
                    };
                    if let Ok(id) = instance.create_session(ws, SessionKind::Shell, mode) {
                        sessions.push(id);
                    }
                }
                2 if !sessions.is_empty() => {
                    let at = rng.below(sessions.len() as u64) as usize;
                    let id = sessions.remove(at);
                    let _ = instance.close_session(id);
                }
                3 if !sessions.is_empty() => {
                    let id = sessions[rng.below(sessions.len() as u64) as usize];
                    if let Some(session) = instance.session_mut(id) {
                        session.title = format!("step {step}");
                    }
                }
                _ => {
                    // Closing a workspace takes its sessions with it, which is
                    // the operation most likely to leave a dangling reference.
                    if !workspaces.is_empty() {
                        let at = rng.below(workspaces.len() as u64) as usize;
                        let ws = workspaces.remove(at);
                        let _ = instance.close_workspace(ws);
                        sessions.retain(|id| instance.session(*id).is_some());
                    }
                }
            }

            let violations = check(&instance);
            assert!(
                violations.is_empty(),
                "seed {seed}, step {step}: {}",
                violations
                    .iter()
                    .map(std::string::ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("; ")
            );
        }
    }
}

#[test]
fn closing_a_workspace_leaves_nothing_pointing_into_it() {
    // The operation most likely to leave a dangling reference, checked on its
    // own so a failure says which operation rather than which seed.
    let mut instance = Instance::default();
    let ws = instance.open_workspace("/tmp/a").expect("workspace");
    for _ in 0..5 {
        instance
            .create_session(ws, SessionKind::Shell, SessionMode::Pty)
            .expect("session");
    }
    assert_ok(&instance);
    let _ = instance.close_workspace(ws);
    assert_ok(&instance);
    assert!(
        instance.sessions().is_empty(),
        "its sessions went with it rather than outliving their workspace"
    );
}

#[test]
fn the_same_path_opened_twice_is_one_workspace() {
    // Identity is the canonical path, so opening the same directory twice must
    // not produce two workspaces whose sessions cannot see each other.
    let mut instance = Instance::default();
    let first = instance.open_workspace("/tmp/same").expect("first");
    let second = instance.open_workspace("/tmp/same").expect("second");
    assert_eq!(first, second);
    assert_ok(&instance);
}

#[test]
fn a_git_worktree_is_its_own_workspace() {
    // 05 §7: `git worktree add ../feature-x` is how people run two agents on
    // two branches at once. Collapsing worktrees into one workspace by repo
    // root would have those two agents share a layout, a history scope and a
    // name.
    let mut instance = Instance::default();
    let main = instance.open_workspace("/tmp/repo").expect("main");
    let worktree = instance
        .open_workspace("/tmp/repo-feature-x")
        .expect("worktree");
    assert_ne!(main, worktree);
    assert_ok(&instance);
}
