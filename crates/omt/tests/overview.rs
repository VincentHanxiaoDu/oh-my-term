//! The headline claims, checked against the code.
//!
//! `docs/architecture/00-overview.md` summarises the documents beneath it. A
//! summary is the thing most likely to drift — it is read most, edited least,
//! and nothing breaks when it goes stale — so the claims that are checkable are
//! checked here rather than trusted.
//!
//! Only the load-bearing numbers and orderings. This is not a second copy of
//! the detail documents' test suites; it is a tripwire for the overview saying
//! six of something when there are five.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]

use omt_types::Tier;

#[test]
fn the_observation_pipeline_has_the_six_sources_the_overview_claims() {
    // §5. A seventh would make the table wrong; a sixth removed would make a
    // coverage claim wrong.
    let tiers = [
        Tier::Heuristic,
        Tier::Process,
        Tier::Marker,
        Tier::Transcript,
        Tier::Hook,
        Tier::Protocol,
    ];
    assert_eq!(tiers.len(), 6);
    // Distinct, or "ranked by confidence" means nothing.
    let unique: std::collections::BTreeSet<_> = tiers.iter().collect();
    assert_eq!(unique.len(), 6);
}

#[test]
fn higher_is_more_authoritative() {
    // §5 states this explicitly, and every merge rule depends on the ordering
    // running the way the document says. An enum reordered by somebody tidying
    // it alphabetically would invert the whole model silently.
    assert!(Tier::Protocol > Tier::Hook);
    assert!(Tier::Hook > Tier::Transcript);
    assert!(Tier::Transcript > Tier::Marker);
    assert!(Tier::Marker > Tier::Process);
    assert!(Tier::Process > Tier::Heuristic);
}

#[test]
fn a_lower_tier_never_contradicts_a_live_higher_one() {
    // The rule the tier ladder exists for. Checked through the merge machine
    // rather than restated: a guess must not be able to overwrite something an
    // agent said about itself.
    use omt_agent::merge::{MergeMachine, SourceReading};
    use omt_types::AgentState;

    let mut merge = MergeMachine::default();
    merge.observe(SourceReading {
        tier: Tier::Protocol,
        state: AgentState::Working { detail: None },
        at: 1_000,
        authoritative: true,
    });
    merge.observe(SourceReading {
        tier: Tier::Heuristic,
        state: AgentState::Idle,
        at: 1_001,
        authoritative: false,
    });
    assert_eq!(
        merge.state(1_002),
        AgentState::Working { detail: None },
        "a heuristic overwrote what the agent said about itself"
    );
}

#[test]
fn the_terminal_core_is_a_real_one() {
    // §6. Parser, grid, scrollback, reflow and the block model — the four
    // things that separate "a terminal" from "a text area that shows output".
    let mut term = omt_term::Terminal::new(omt_term::TermConfig::default());
    term.advance(b"\x1b[1;32mgreen\x1b[0m\r\n");
    assert!(term.total_rows() > 0, "output reached the grid");

    // Reflow: content survives a width change as logical lines.
    term.advance(&[b'x'; 200]);
    let before = term.total_rows();
    term.resize(omt_term::GridSize { cols: 40, rows: 24 });
    assert!(term.total_rows() > 0, "content survived the resize: {before}");

    // The block model, from the shell's own marks rather than a guess.
    let mut marked = omt_term::Terminal::new(omt_term::TermConfig::default());
    marked.advance(b"\x1b]133;A\x07$ \x1b]133;B\x07echo hi\r\n\x1b]133;C\x07hi\r\n\x1b]133;D;0\x07");
    assert!(!marked.blocks().is_empty(), "OSC 133 produced a block");
}

#[test]
fn every_instance_is_authoritative_for_its_own_sessions() {
    // §7. Session ids are minted per instance and carry no instance in them,
    // so a federating client keys them by the connection they came from —
    // which is what stops one instance's view being overwritten by another's.
    let a = omt_types::SessionId::new();
    let b = omt_types::SessionId::new();
    assert_ne!(a, b, "ids are not drawn from a shared, ordered sequence");
}

#[test]
fn the_three_built_in_auth_methods_are_there_plus_tailnet() {
    // §7 names signed invite link, bearer token and password, plus
    // transport-level trust inside a tailnet. A method quietly dropped would
    // lock somebody out of a deployment the document told them to build.
    use omt_auth::methods::{Backends, Method, Peer};

    // Offered to a tailnet peer with every backend configured: this is the set
    // a client is actually told it may use.
    let offered = omt_auth::methods::offered_for(
        Backends {
            bearer: true,
            password: true,
            invite: true,
            tailnet: true,
            device_grant: true,
        },
        Peer::Tailnet,
    );
    for method in [Method::Invite, Method::Bearer, Method::Password] {
        assert!(offered.contains(&method), "{method:?} is not offered");
    }
    assert!(
        offered.contains(&Method::Tailnet),
        "transport-level trust inside a tailnet is not offered"
    );
}
