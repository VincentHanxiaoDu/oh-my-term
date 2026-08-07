//! Collecting adapters, including ones omt did not write.

use std::collections::BTreeMap;

use omt_types::AgentKind;

use crate::adapter::AgentAdapter;
use crate::agents::{ClaudeCode, Codex, Copilot, Cursor, Gemini, GenericAcp, HeuristicFloor};

/// Every adapter available to an instance.
///
/// A set rather than a match on [`AgentKind`], so a third party can add an
/// agent omt has never heard of without editing anything here. The registry is
/// the extension point; the enum is only a label.
#[derive(Default)]
pub struct AdapterSet {
    by_kind: BTreeMap<AgentKind, Box<dyn AgentAdapter>>,
}

impl AdapterSet {
    /// An empty set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an adapter, replacing any adapter for the same agent.
    ///
    /// Replacing rather than refusing: overriding a built-in adapter with a
    /// better one is the point of the extension point, and a registry that
    /// refused would make that impossible without a fork.
    pub fn insert(&mut self, adapter: Box<dyn AgentAdapter>) -> Option<Box<dyn AgentAdapter>> {
        self.by_kind.insert(adapter.kind(), adapter)
    }

    /// The adapter for an agent.
    #[must_use]
    pub fn get(&self, kind: AgentKind) -> Option<&dyn AgentAdapter> {
        self.by_kind.get(&kind).map(std::convert::AsRef::as_ref)
    }

    /// Every adapter, in a stable order.
    #[must_use]
    pub fn all(&self) -> Vec<&dyn AgentAdapter> {
        self.by_kind
            .values()
            .map(std::convert::AsRef::as_ref)
            .collect()
    }

    /// How many.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_kind.len()
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_kind.is_empty()
    }
}

/// The adapters omt ships.
#[must_use]
pub fn builtin() -> AdapterSet {
    let mut set = AdapterSet::new();
    set.insert(Box::new(ClaudeCode));
    set.insert(Box::new(Codex));
    set.insert(Box::new(Copilot));
    set.insert(Box::new(Cursor));

    // The generic sets go in first, and the dedicated adapters after — because
    // `insert` replaces by kind, and an agent with a dedicated adapter must get
    // it rather than the generic one that also claims its kind. Written in this
    // order rather than trusted to a comment: the test below fails if it slips.
    for kind in GenericAcp::COVERS {
        set.insert(Box::new(GenericAcp::new(*kind)));
    }
    for kind in HeuristicFloor::COVERS {
        set.insert(Box::new(HeuristicFloor::new(*kind)));
    }
    set.insert(Box::new(Gemini));
    set
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;
    use crate::adapter::{Fingerprint, Interrupt, SpawnCtx};
    use omt_events::AgentPayload;
    use omt_types::Tier;
    use std::ffi::OsString;

    /// An adapter written entirely outside this crate, using only its public
    /// surface. If this stops compiling, the extension point has closed.
    struct ThirdParty;

    impl AgentAdapter for ThirdParty {
        fn kind(&self) -> AgentKind {
            AgentKind::Unknown
        }
        fn fingerprint(&self) -> Fingerprint {
            Fingerprint {
                exe_names: vec!["my-agent"],
                ..Fingerprint::default()
            }
        }
        fn spawn_env(&self, _ctx: &SpawnCtx) -> Vec<(OsString, OsString)> {
            vec![(OsString::from("MY_AGENT"), OsString::from("1"))]
        }
        fn best_tier(&self) -> Tier {
            Tier::Protocol
        }
        fn normalize(
            &self,
            _event: &str,
            _payload: &serde_json::Value,
        ) -> Result<Vec<AgentPayload>, crate::adapter::AdapterError> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn the_builtin_set_covers_every_agent_in_the_matrix() {
        let set = builtin();
        // Every agent the architecture's matrix names. Listed here rather
        // than derived, so adding a row to the docs and forgetting the adapter
        // fails a test instead of shipping a gap.
        for kind in [
            AgentKind::ClaudeCode,
            AgentKind::Codex,
            AgentKind::Copilot,
            AgentKind::Cursor,
            AgentKind::Opencode,
            AgentKind::GeminiCli,
            AgentKind::QwenCode,
            AgentKind::Goose,
            AgentKind::Aider,
            AgentKind::Amp,
            AgentKind::Crush,
        ] {
            assert!(set.get(kind).is_some(), "no adapter for {kind:?}");
        }
    }

    #[test]
    fn an_adapter_can_be_written_from_outside_this_crate() {
        // Only the public surface is used above. A trait with a private
        // requirement would fail to compile here rather than at some third
        // party's desk.
        let mut set = AdapterSet::new();
        set.insert(Box::new(ThirdParty));
        let a = set.get(AgentKind::Unknown).expect("registered");
        assert_eq!(a.best_tier(), Tier::Protocol);
        assert_eq!(a.interrupt(), Interrupt::ControlC, "the default applies");
    }

    #[test]
    fn a_third_party_adapter_can_replace_a_built_in_one() {
        // The point of the extension point: overriding must not need a fork.
        struct BetterClaude;
        impl AgentAdapter for BetterClaude {
            fn kind(&self) -> AgentKind {
                AgentKind::ClaudeCode
            }
            fn fingerprint(&self) -> Fingerprint {
                Fingerprint::default()
            }
            fn spawn_env(&self, _ctx: &SpawnCtx) -> Vec<(OsString, OsString)> {
                Vec::new()
            }
            fn best_tier(&self) -> Tier {
                Tier::Protocol
            }
            fn normalize(
                &self,
                _event: &str,
                _payload: &serde_json::Value,
            ) -> Result<Vec<AgentPayload>, crate::adapter::AdapterError> {
                Ok(Vec::new())
            }
        }
        let mut set = builtin();
        let before = set.len();
        let replaced = set.insert(Box::new(BetterClaude));
        assert!(replaced.is_some(), "the built-in came back out");
        assert_eq!(set.len(), before, "replaced, not added alongside");
        assert_eq!(
            set.get(AgentKind::ClaudeCode).expect("present").best_tier(),
            Tier::Protocol
        );
    }

    #[test]
    fn an_agent_omt_has_never_heard_of_still_gets_the_floor() {
        // The difference between "omt supports these eleven agents" and "omt
        // supports agents". A CLI released next month gets activity detection
        // and an interrupt — the whole remote surface at heuristic tier — and
        // says it is guessing, rather than getting no adapter and therefore
        // nothing at all.
        let set = builtin();
        let unknown = set
            .get(AgentKind::Unknown)
            .expect("an unrecognised agent has no adapter");
        assert_eq!(unknown.best_tier(), Tier::Heuristic);
        assert_eq!(
            unknown.interrupt(),
            Interrupt::ControlC,
            "the one control a heuristic-tier agent has"
        );
    }

    #[test]
    fn the_floor_cannot_be_talked_into_claiming_structure() {
        // A heuristic source that emitted a card would be a card assembled from
        // pixels, which is the failure the whole tier ladder exists to prevent.
        let set = builtin();
        let unknown = set.get(AgentKind::Unknown).expect("unknown");
        let payloads = unknown
            .normalize("PreToolUse", &serde_json::json!({ "tool_name": "bash" }))
            .unwrap_or_default();
        assert!(
            !payloads
                .iter()
                .any(|p| matches!(p, AgentPayload::Question { .. })),
            "the floor produced a structured question"
        );
    }

    #[test]
    fn a_dedicated_adapter_wins_over_the_generic_one_that_covers_it() {
        // `GenericAcp` also claims Gemini. If it were inserted last it would
        // replace the dedicated adapter, and Gemini's own hook names —
        // BeforeTool, AfterTool — would stop being understood, silently.
        let set = builtin();
        let gemini = set.get(AgentKind::GeminiCli).expect("gemini");
        assert!(
            gemini
                .normalize("BeforeTool", &serde_json::json!({}))
                .is_ok(),
            "the generic ACP adapter replaced the dedicated one"
        );
    }

    #[test]
    fn adapters_enumerate_in_a_stable_order() {
        // Detection walks these, and an order that varied between runs would
        // make a tie break differently on each start.
        let first: Vec<AgentKind> = builtin().all().iter().map(|a| a.kind()).collect();
        let second: Vec<AgentKind> = builtin().all().iter().map(|a| a.kind()).collect();
        assert_eq!(first, second);
    }

    #[test]
    fn every_builtin_adapter_states_a_tier_it_can_actually_reach() {
        // A floor agent claiming Hook would turn on a transcript view that can
        // never be populated.
        let set = builtin();
        assert_eq!(
            set.get(AgentKind::Aider).expect("aider").best_tier(),
            Tier::Heuristic
        );
        assert_eq!(
            set.get(AgentKind::ClaudeCode).expect("claude").best_tier(),
            Tier::Hook
        );
        assert_eq!(
            set.get(AgentKind::Opencode).expect("opencode").best_tier(),
            Tier::Protocol
        );
    }

    #[test]
    fn every_adapter_injects_the_session_correlation() {
        // Without it, a hook or plugin has to guess which pane it belongs to —
        // the whole class of heuristics this removes.
        let ctx = SpawnCtx {
            session: "s-42".into(),
            ..SpawnCtx::default()
        };
        for adapter in builtin().all() {
            let env = adapter.spawn_env(&ctx);
            assert!(
                env.iter().any(|(k, v)| k == "OMT_SESSION" && v == "s-42"),
                "{:?} injects nothing to correlate with",
                adapter.kind()
            );
        }
    }
}
