//! The one invariant that must hold for every adapter, present and future.
//!
//! Stated once, over the whole registry, rather than as a test per adapter —
//! because the failure this guards against is somebody adding the eleventh
//! adapter and not thinking about it.

use omt_agent_adapters::{
    AgentAdapter, ClaudeCode, GenericAcp, HeuristicFloor, Observation, ScreenSignals, SpawnCtx,
    builtin, detect, guess_activity, may_emit_structured,
};
use omt_events::{ActivityGuess, AgentPayload};
use omt_types::{AgentKind, Tier};

#[test]
fn no_adapter_reports_a_tier_it_could_not_have_reached() {
    // The ladder's premise: the tier is a claim about *how omt knows*, and a
    // claim of Hook or Protocol means something told us. An adapter that
    // overstated this would license a card the user taps "Allow" on, built
    // from a guess.
    for adapter in builtin().all() {
        let tier = adapter.best_tier();
        let has_structured_source = matches!(tier, Tier::Hook | Tier::Protocol | Tier::Transcript);
        assert_eq!(
            has_structured_source,
            tier.may_emit_structured_content(),
            "{:?} claims {tier:?}, which disagrees with what that tier permits",
            adapter.kind()
        );
    }
}

#[test]
fn a_floor_agent_may_never_emit_structured_content() {
    for kind in HeuristicFloor::COVERS {
        let a = HeuristicFloor::new(*kind);
        assert!(
            !may_emit_structured(a.best_tier()),
            "{kind:?} is at the floor and must stay there"
        );
    }
}

#[test]
fn every_heuristic_guess_stays_unstructured() {
    // There is deliberately no code path from screen text to a tool call.
    for signals in [
        ScreenSignals {
            output_in_last_second: true,
            ..ScreenSignals::default()
        },
        ScreenSignals {
            bell_recently: true,
            ..ScreenSignals::default()
        },
        ScreenSignals {
            quiet_for_secs: 30,
            ..ScreenSignals::default()
        },
        ScreenSignals::default(),
    ] {
        let guess = guess_activity(&signals);
        assert!(
            !AgentPayload::Activity { state: guess }.is_structured(),
            "{signals:?} produced structure"
        );
        assert!(matches!(
            guess,
            ActivityGuess::Busy
                | ActivityGuess::Idle
                | ActivityGuess::NeedsAttention
                | ActivityGuess::Unknown
        ));
    }
}

#[test]
fn detection_by_process_name_never_reaches_a_structured_tier() {
    // Anyone may name a script after an agent. Being recognized that way must
    // buy activity and nothing more, however deep the adapter itself goes.
    for (exe, expected) in [
        ("claude", AgentKind::ClaudeCode),
        ("opencode", AgentKind::Opencode),
        ("aider", AgentKind::Aider),
    ] {
        let set = builtin();
        let adapters = set.all();
        let d = detect(
            &adapters,
            &Observation {
                exe_name: Some(exe.to_owned()),
                ..Observation::default()
            },
        );
        assert_eq!(d.agent, expected, "{exe}");
        assert_eq!(d.tier, Tier::Process, "{exe}");
        assert!(
            !may_emit_structured(d.tier),
            "a name is a resemblance, not a statement"
        );
    }
}

#[test]
fn every_adapter_can_be_interrupted_somehow() {
    // For a floor agent this is the entire remote control surface, so "no way
    // to stop it" is not an acceptable answer for anything in the registry.
    for adapter in builtin().all() {
        let i = adapter.interrupt();
        assert!(
            i.bytes().is_some() || i == omt_agent_adapters::Interrupt::Native,
            "{:?} cannot be stopped",
            adapter.kind()
        );
    }
}

#[test]
fn an_adapter_that_speaks_a_protocol_can_still_run_the_users_own_cli() {
    // Supporting ACP must never cost the user the actual CLI they installed —
    // a native session is not the same program.
    for adapter in builtin().all() {
        assert!(
            adapter.supported_modes().pty,
            "{:?} dropped pty support",
            adapter.kind()
        );
    }
}

#[test]
fn an_agent_declaring_native_mode_says_how_to_start_it() {
    // Declaring a mode with no way to enter it would fail at the moment a user
    // picked it, rather than when it was declared.
    let ctx = SpawnCtx::default();
    for adapter in builtin().all() {
        if adapter.supported_modes().native {
            assert!(
                adapter.acp_spawn(&ctx).is_some(),
                "{:?} claims native mode with no spawn",
                adapter.kind()
            );
        }
    }
}

#[test]
fn normalizing_an_unknown_event_never_invents_a_payload() {
    // A version that moved must surface as an error, not as a plausible event
    // nobody can trace.
    for adapter in builtin().all() {
        let result = adapter.normalize("DefinitelyNotARealEvent", &serde_json::json!({}));
        assert!(
            result.is_err() || result.as_ref().is_ok_and(std::vec::Vec::is_empty),
            "{:?} invented {:?}",
            adapter.kind(),
            result
        );
    }
}

#[test]
fn the_deepest_adapter_and_the_shallowest_agree_on_the_trait() {
    // The reason the ACP adapter was built second: a trait shaped by one agent
    // fits one agent. These three have nothing in common except this trait.
    let deep: &dyn AgentAdapter = &ClaudeCode;
    let acp = GenericAcp::new(AgentKind::Opencode);
    let floor = HeuristicFloor::new(AgentKind::Crush);
    let all: [&dyn AgentAdapter; 3] = [deep, &acp, &floor];

    let ctx = SpawnCtx {
        instance: "i".into(),
        session: "s".into(),
        socket: "/tmp/s".into(),
        ..SpawnCtx::default()
    };
    for a in all {
        assert!(!a.spawn_env(&ctx).is_empty(), "{:?}", a.kind());
        assert!(!a.fingerprint().exe_names.is_empty(), "{:?}", a.kind());
    }
    assert_eq!(deep.best_tier(), Tier::Hook);
    assert_eq!(acp.best_tier(), Tier::Protocol);
    assert_eq!(floor.best_tier(), Tier::Heuristic);
}
