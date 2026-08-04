//! Running one prompt across several agents, each in its own worktree.
//!
//! The model already supports this and nothing had to change for it:
//! `WorkspaceId` is derived from a canonical path, so N worktrees are N
//! workspaces by construction. What was missing is the fan-out itself, a way to
//! compare the results, and a way to say which one won.
//!
//! What this deliberately does **not** do is merge. Choosing a winner records a
//! decision; performing the merge is a git operation with its own failure modes
//! and its own confirmation, and folding it in here would make "compare these"
//! and "rewrite my branch" one button.

use std::collections::BTreeMap;

use omt_types::{AgentKind, Timestamp, WorkspaceId};
use serde::{Deserialize, Serialize};

/// One agent's attempt at the prompt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Arm {
    /// Which agent is running it.
    pub agent: AgentKind,
    /// The worktree it runs in, which is also its workspace.
    pub worktree: String,
    /// The branch that worktree is on.
    pub branch: String,
    /// Where it has got to.
    pub state: ArmState,
}

/// Where one arm has got to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ArmState {
    /// The worktree exists; the agent has not started.
    Prepared,
    /// The agent is working.
    Running,
    /// It finished and left changes.
    Finished {
        /// How many files it touched, for a summary that does not need a diff.
        files_changed: u32,
    },
    /// It failed or was stopped.
    Failed {
        /// Why, in the words of whatever failed.
        reason: String,
    },
}

impl ArmState {
    /// Whether this arm can still change.
    #[must_use]
    pub const fn is_settled(&self) -> bool {
        matches!(self, Self::Finished { .. } | Self::Failed { .. })
    }

    /// Whether it produced something worth comparing.
    #[must_use]
    pub const fn is_comparable(&self) -> bool {
        matches!(self, Self::Finished { .. })
    }
}

/// Why a fan-out could not be set up.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FanoutError {
    /// Fewer than two agents is not a comparison.
    #[error("a fan-out needs at least two agents; {given} were given")]
    TooFew {
        /// How many were given.
        given: usize,
    },
    /// Two arms would share a worktree.
    #[error("`{worktree}` was given to more than one agent")]
    SharedWorktree {
        /// The contested path.
        worktree: String,
    },
    /// No such arm.
    #[error("no arm for {0:?}")]
    NoArm(AgentKind),
    /// A winner was chosen before there was anything to compare.
    #[error("nothing has finished yet")]
    NothingFinished,
}

/// One prompt, several agents, one winner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fanout {
    /// The prompt every arm was given, verbatim.
    pub prompt: String,
    /// The commit every worktree started from, so a comparison is fair.
    pub base: String,
    /// Each arm, keyed by the agent running it.
    arms: BTreeMap<AgentKind, Arm>,
    /// Which arm was chosen, once one was.
    chosen: Option<AgentKind>,
    /// When it started.
    pub created_at: Timestamp,
}

impl Fanout {
    /// Set up a fan-out.
    ///
    /// # Errors
    /// Fails with fewer than two agents — one agent is not a comparison — or if
    /// two arms would share a worktree, which would have them overwrite each
    /// other's work and produce a comparison of one result against itself.
    pub fn new(
        prompt: &str,
        base: &str,
        arms: Vec<(AgentKind, String, String)>,
    ) -> Result<Self, FanoutError> {
        if arms.len() < 2 {
            return Err(FanoutError::TooFew { given: arms.len() });
        }

        let mut seen: BTreeMap<String, ()> = BTreeMap::new();
        let mut by_agent = BTreeMap::new();
        for (agent, worktree, branch) in arms {
            if seen.insert(worktree.clone(), ()).is_some() {
                return Err(FanoutError::SharedWorktree { worktree });
            }
            by_agent.insert(
                agent,
                Arm {
                    agent,
                    worktree,
                    branch,
                    state: ArmState::Prepared,
                },
            );
        }

        Ok(Self {
            prompt: prompt.to_owned(),
            base: base.to_owned(),
            arms: by_agent,
            chosen: None,
            created_at: Timestamp::now(),
        })
    }

    /// Every arm, in a stable order.
    #[must_use]
    pub fn arms(&self) -> Vec<&Arm> {
        self.arms.values().collect()
    }

    /// One arm.
    #[must_use]
    pub fn arm(&self, agent: AgentKind) -> Option<&Arm> {
        self.arms.get(&agent)
    }

    /// The workspace id an arm's worktree resolves to.
    ///
    /// Derived from the path, which is why N worktrees are N workspaces without
    /// anything having to arrange it.
    #[must_use]
    pub fn workspace_of(&self, agent: AgentKind) -> Option<WorkspaceId> {
        self.arms
            .get(&agent)
            .map(|a| WorkspaceId::from_canonical_path(&a.worktree))
    }

    /// Record where an arm has got to.
    ///
    /// # Errors
    /// Fails if there is no such arm.
    pub fn set_state(&mut self, agent: AgentKind, state: ArmState) -> Result<(), FanoutError> {
        let arm = self.arms.get_mut(&agent).ok_or(FanoutError::NoArm(agent))?;
        arm.state = state;
        Ok(())
    }

    /// Whether every arm has settled.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.arms.values().all(|a| a.state.is_settled())
    }

    /// The arms worth comparing.
    ///
    /// A failed arm is excluded rather than shown as an empty result: a user
    /// scanning for the best answer should not have to work out which of five
    /// panes is blank because the agent crashed.
    #[must_use]
    pub fn comparable(&self) -> Vec<&Arm> {
        self.arms
            .values()
            .filter(|a| a.state.is_comparable())
            .collect()
    }

    /// Choose the winner.
    ///
    /// Records a decision and nothing more. The merge is a separate git
    /// operation with its own failure modes and its own confirmation — folding
    /// it in here would make "compare these" and "rewrite my branch" one
    /// button.
    ///
    /// # Errors
    /// Fails if there is no such arm, or if that arm produced nothing.
    pub fn choose(&mut self, agent: AgentKind) -> Result<&Arm, FanoutError> {
        let arm = self.arms.get(&agent).ok_or(FanoutError::NoArm(agent))?;
        if !arm.state.is_comparable() {
            return Err(FanoutError::NothingFinished);
        }
        self.chosen = Some(agent);
        Ok(arm)
    }

    /// Which arm was chosen, if one was.
    #[must_use]
    pub fn chosen(&self) -> Option<&Arm> {
        self.chosen.and_then(|a| self.arms.get(&a))
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

    fn fanout() -> Fanout {
        Fanout::new(
            "add retries to the http client",
            "abc123",
            vec![
                (
                    AgentKind::ClaudeCode,
                    "/w/.worktrees/claude".to_owned(),
                    "try/claude".to_owned(),
                ),
                (
                    AgentKind::Codex,
                    "/w/.worktrees/codex".to_owned(),
                    "try/codex".to_owned(),
                ),
            ],
        )
        .expect("fanout")
    }

    #[test]
    fn each_arm_is_its_own_workspace_without_anything_arranging_it() {
        // WorkspaceId is derived from the path, which is why the model already
        // supported this and nothing had to change.
        let f = fanout();
        let a = f.workspace_of(AgentKind::ClaudeCode).expect("arm");
        let b = f.workspace_of(AgentKind::Codex).expect("arm");
        assert_ne!(a, b);
        assert_eq!(a, WorkspaceId::from_canonical_path("/w/.worktrees/claude"));
    }

    #[test]
    fn one_agent_is_not_a_comparison() {
        let err = Fanout::new(
            "do a thing",
            "abc",
            vec![(AgentKind::ClaudeCode, "/w/a".to_owned(), "b".to_owned())],
        )
        .expect_err("must refuse");
        assert!(matches!(err, FanoutError::TooFew { given: 1 }), "{err:?}");
    }

    #[test]
    fn two_arms_cannot_share_a_worktree() {
        // They would overwrite each other and produce a comparison of one
        // result against itself.
        let err = Fanout::new(
            "do a thing",
            "abc",
            vec![
                (AgentKind::ClaudeCode, "/w/same".to_owned(), "a".to_owned()),
                (AgentKind::Codex, "/w/same".to_owned(), "b".to_owned()),
            ],
        )
        .expect_err("must refuse");
        assert!(matches!(err, FanoutError::SharedWorktree { .. }), "{err:?}");
    }

    #[test]
    fn every_arm_starts_from_the_same_commit() {
        // Otherwise the comparison is between different starting points and
        // says nothing about the agents.
        let f = fanout();
        assert_eq!(f.base, "abc123");
        assert!(f.arms().iter().all(|a| a.state == ArmState::Prepared));
    }

    #[test]
    fn every_arm_gets_the_prompt_verbatim() {
        let f = fanout();
        assert_eq!(f.prompt, "add retries to the http client");
    }

    #[test]
    fn a_failed_arm_is_not_offered_for_comparison() {
        // A user scanning for the best answer should not have to work out
        // which pane is blank because the agent crashed.
        let mut f = fanout();
        f.set_state(
            AgentKind::ClaudeCode,
            ArmState::Finished { files_changed: 3 },
        )
        .expect("set");
        f.set_state(
            AgentKind::Codex,
            ArmState::Failed {
                reason: "the agent exited".to_owned(),
            },
        )
        .expect("set");

        assert_eq!(f.comparable().len(), 1);
        assert_eq!(f.comparable()[0].agent, AgentKind::ClaudeCode);
        assert!(f.is_complete(), "settled is not the same as successful");
    }

    #[test]
    fn a_running_fanout_is_not_complete() {
        let mut f = fanout();
        f.set_state(AgentKind::ClaudeCode, ArmState::Running)
            .expect("set");
        assert!(!f.is_complete());
    }

    #[test]
    fn an_arm_that_produced_nothing_cannot_be_chosen() {
        let mut f = fanout();
        f.set_state(
            AgentKind::Codex,
            ArmState::Failed {
                reason: "crashed".to_owned(),
            },
        )
        .expect("set");
        assert!(matches!(
            f.choose(AgentKind::Codex),
            Err(FanoutError::NothingFinished)
        ));
        assert!(f.chosen().is_none());
    }

    #[test]
    fn choosing_records_a_decision_and_does_not_merge() {
        // Merging is a git operation with its own failure modes and its own
        // confirmation. Folding it in here would make "compare these" and
        // "rewrite my branch" one button.
        let mut f = fanout();
        f.set_state(
            AgentKind::ClaudeCode,
            ArmState::Finished { files_changed: 7 },
        )
        .expect("set");
        let winner = f.choose(AgentKind::ClaudeCode).expect("choose").clone();
        assert_eq!(winner.branch, "try/claude");
        assert_eq!(f.chosen().map(|a| a.agent), Some(AgentKind::ClaudeCode));
        // The losing worktree is untouched: nothing was merged, deleted or
        // rebased by choosing.
        assert_eq!(
            f.arm(AgentKind::Codex).map(|a| a.state.clone()),
            Some(ArmState::Prepared)
        );
    }

    #[test]
    fn a_choice_can_be_changed() {
        // A user comparing five results changes their mind; a decision that
        // could not be revised would push them to be sure before they had
        // looked.
        let mut f = fanout();
        for agent in [AgentKind::ClaudeCode, AgentKind::Codex] {
            f.set_state(agent, ArmState::Finished { files_changed: 1 })
                .expect("set");
        }
        f.choose(AgentKind::ClaudeCode).expect("first");
        f.choose(AgentKind::Codex).expect("second");
        assert_eq!(f.chosen().map(|a| a.agent), Some(AgentKind::Codex));
    }

    #[test]
    fn an_unknown_arm_is_an_error_rather_than_silently_ignored() {
        let mut f = fanout();
        assert!(matches!(
            f.set_state(AgentKind::Aider, ArmState::Running),
            Err(FanoutError::NoArm(_))
        ));
    }

    #[test]
    fn the_order_of_arms_is_stable() {
        // A comparison view whose panes reshuffle is one the user misreads.
        let f = fanout();
        let first: Vec<AgentKind> = f.arms().iter().map(|a| a.agent).collect();
        let second: Vec<AgentKind> = f.arms().iter().map(|a| a.agent).collect();
        assert_eq!(first, second);
    }
}
