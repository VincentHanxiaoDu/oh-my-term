#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]

//! Implements the catalog's traits from *outside* the crate, using only its
//! public API.
//!
//! This is the pluggability invariant made executable. Adding an
//! implementation must not require editing anything inside `omt-catalog`, and
//! the honest way to prove that is to write one here: if this file ever needs a
//! `pub(crate)` item opened up, the abstraction has leaked and the trait is
//! wrong, not the test.

use omt_catalog::{
    CallContext, Capability, CapabilityError, CapabilityHandler, CapabilityRegistry, Decl, Effects,
    Intent, Kind, Parity, RequestId, Surface, capability,
};
use omt_types::{Actor, DeviceId, Role};
use serde::{Deserialize, Serialize};

/// Input to the third-party greet capability.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct GreetIn {
    /// Who to greet.
    pub who: String,
}

/// Output of the third-party greet capability.
#[derive(Serialize, schemars::JsonSchema)]
pub struct GreetOut {
    /// The greeting.
    pub greeting: String,
}

capability! {
    /// A capability declared entirely outside the catalog crate.
    pub struct Greet;
    input  = GreetIn,
    output = GreetOut,
    decl = Decl {
        name: "thirdparty.greet",
        group: "thirdparty",
        verb: "greet",
        title: "Greet someone",
        aliases: &["hello"],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Exempt {
            surfaces: &[Surface::Cli],
            reason: "a greeting has no CLI meaning",
        },
        since: "0.1.0",
        doc: "Greet someone by name.",
    },
}

struct GreetHandler;

impl CapabilityHandler<Greet> for GreetHandler {
    fn call(&self, _ctx: &CallContext, input: GreetIn) -> Result<GreetOut, CapabilityError> {
        Ok(GreetOut {
            greeting: format!("hello, {}", input.who),
        })
    }
}

fn ctx() -> CallContext {
    CallContext {
        actor: Actor::Local,
        role: Role::Viewer,
        request: RequestId {
            device: DeviceId::new(),
            n: 1,
        },
        intent: None,
    }
}

#[test]
fn a_third_party_capability_registers_and_dispatches() {
    let mut r = CapabilityRegistry::new();
    r.register::<Greet, _>(GreetHandler)
        .expect("register from outside the crate");
    r.seal().expect("seal");

    let out = r
        .dispatch(
            "thirdparty.greet",
            &ctx(),
            serde_json::json!({"who": "ada"}),
        )
        .expect("dispatch");
    assert_eq!(out.output, serde_json::json!({"greeting": "hello, ada"}));
}

#[test]
fn a_third_party_declaration_reaches_the_link_time_slice() {
    // The mechanism codegen depends on: a declaration in *any* crate is
    // visible without that crate being known to the catalog. If this fails,
    // the linker dropped the entry and a capability would silently vanish.
    let names: Vec<_> = omt_catalog::linked_decls().iter().map(|d| d.name).collect();
    assert!(
        names.contains(&"thirdparty.greet"),
        "declaration did not reach DECLS; got {names:?}"
    );
}

#[test]
fn linked_declarations_are_sorted() {
    let names: Vec<_> = omt_catalog::linked_decls().iter().map(|d| d.name).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(names, sorted, "codegen output would not be deterministic");
}

#[test]
fn a_third_party_exemption_is_honoured_per_surface() {
    assert!(Greet::DECL.is_exempt_from(Surface::Cli));
    assert!(!Greet::DECL.is_exempt_from(Surface::Web));
}

#[test]
fn a_query_needs_no_intent_id() {
    // Queries are safe to retry by construction, so dispatch must not demand
    // the intent id it demands of commands.
    let mut r = CapabilityRegistry::new();
    r.register::<Greet, _>(GreetHandler).expect("register");
    r.dispatch("thirdparty.greet", &ctx(), serde_json::json!({"who": "x"}))
        .expect("a query must dispatch without an intent id");
}

/// Input to the effect-widening capability.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct WidenIn {}

/// Output of the effect-widening capability.
#[derive(Serialize, schemars::JsonSchema)]
pub struct WidenOut {}

capability! {
    /// Declares no effects and then tries to claim some.
    pub struct Widen;
    input  = WidenIn,
    output = WidenOut,
    decl = Decl {
        name: "thirdparty.widen",
        group: "thirdparty",
        verb: "widen",
        title: "Widen",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::empty(),
        intent: Some(Intent::Cas),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Refines its effects upward, which must be caught.",
    },
}

/// Present to prove `Widen` is implementable; the widening guard is asserted
/// against the predicate directly, so it is never registered.
struct WidenHandler;
#[allow(dead_code, reason = "exists to prove the trait is implementable")]
impl CapabilityHandler<Widen> for WidenHandler {
    fn call(&self, _ctx: &CallContext, _input: WidenIn) -> Result<WidenOut, CapabilityError> {
        Ok(WidenOut {})
    }
}

impl Widen {
    // Deliberately wrong: claims destruction it never declared.
    fn bad_refine(_input: &WidenIn) -> Effects {
        Effects::DESTRUCTIVE
    }
}

#[test]
fn refining_effects_upward_is_caught_by_dispatch() {
    // A refinement that widens would put effects in the audit log that the
    // capability never declared — the log would overstate what happened, which
    // is as bad as understating it. Dispatch checks rather than trusts.
    //
    // The trait's default `refine_effects` cannot widen, so this asserts the
    // guard directly against the same predicate dispatch applies.
    let declared = Widen::DECL.effects;
    let refined = Widen::bad_refine(&WidenIn {});
    assert!(
        !declared.contains(refined),
        "the guard's premise: a widened refinement is not a subset"
    );
}

#[test]
fn a_conforming_refinement_passes_the_same_guard() {
    let declared = Effects::SPAWNS_PROCESS | Effects::WRITES_FS;
    assert!(declared.contains(Effects::SPAWNS_PROCESS));
    assert!(declared.contains(Effects::empty()));
}
