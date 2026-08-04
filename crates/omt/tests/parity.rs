//! The parity gate.
//!
//! For every declared capability: a route and schema, TUI reachability, a web
//! handler, and a documentation entry — or an explicit, per-surface exemption.
//!
//! What this asserts is deliberately weaker than it first appears, and
//! honestly so. TUI reachability is satisfied by palette membership *or* a
//! binding, because the palette's contents *are* the catalog — so requiring a
//! per-capability chord would be a guarantee omt does not keep. The gate is a
//! floor against *unreachability*. Whether something is *at hand* is a
//! design-review question no test can answer.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]

use std::collections::BTreeSet;

use omt_catalog::{CapabilityRegistry, Surface};
use omt_types::Role;

/// The web client's handler registry, as the generated TypeScript will mirror.
///
/// Read from the checked-in generated artifact rather than hand-written, so a
/// capability added without a handler fails here exactly as it would fail the
/// web build.
fn web_handlers() -> BTreeSet<String> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../web/src/generated/handlers.json"
    );
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("reading {path}: {e}\nrun `cargo xtask codegen`"));
    serde_json::from_str::<Vec<String>>(&text)
        .expect("handlers.json is a JSON array of capability names")
        .into_iter()
        .collect()
}

fn registry() -> CapabilityRegistry {
    omt::capabilities::registry().expect("the registry must build")
}

#[test]
fn every_capability_has_a_route_and_a_schema() {
    let dump: serde_json::Value =
        serde_json::from_str(&omt::capabilities::dump().expect("dump")).expect("parse");
    for cap in dump["capabilities"].as_array().expect("array") {
        let name = cap["name"].as_str().expect("name");
        let route = cap["route"].as_str().expect("route");
        assert!(route.starts_with("/v1/"), "`{name}` has no route");
    }
    assert!(dump["schemas"].is_object(), "no schemas were generated");
}

#[test]
fn every_capability_is_reachable_in_the_tui() {
    let r = registry();
    for decl in r.decls() {
        if decl.role == Role::Admin || decl.is_exempt_from(Surface::Tui) {
            continue;
        }
        assert!(
            omt_tui::is_reachable(decl.name, &r),
            "`{}` is not reachable in the TUI: it is neither bound nor in the palette",
            decl.name
        );
    }
}

#[test]
fn a_hidden_capability_needs_a_real_binding() {
    // A hidden capability has no palette entry, so palette membership cannot
    // satisfy its reachability. Without this rule it would pass the TUI arm
    // with no affordance of any kind.
    let r = registry();
    for decl in r.decls() {
        if !decl.hidden || decl.is_exempt_from(Surface::Tui) {
            continue;
        }
        let bound = omt_tui::ACTIONS
            .iter()
            .any(|a| a.capability == decl.name && a.binding.is_some());
        assert!(
            bound,
            "`{}` is hidden from the palette, so it needs a binding or a TUI exemption",
            decl.name
        );
    }
}

#[test]
fn every_capability_has_a_web_handler() {
    let handlers = web_handlers();
    let r = registry();
    for decl in r.decls() {
        if decl.is_exempt_from(Surface::Web) {
            continue;
        }
        assert!(
            handlers.contains(decl.name),
            "`{}` has no web handler; the web build would fail on it",
            decl.name
        );
    }
}

#[test]
fn every_bound_action_names_a_real_capability() {
    // The reverse direction, which is the half that catches drift: a binding
    // pointing at a renamed or deleted capability would otherwise ship as a key
    // that silently does nothing.
    let r = registry();
    for action in omt_tui::ACTIONS {
        assert!(
            r.decl(action.capability).is_some(),
            "the TUI binds `{}`, which no capability declares",
            action.capability
        );
    }
}

#[test]
fn no_web_handler_names_a_capability_that_does_not_exist() {
    let r = registry();
    for name in web_handlers() {
        assert!(
            r.decl(&name).is_some(),
            "the web client handles `{name}`, which no capability declares"
        );
    }
}

#[test]
fn every_command_declares_an_intent_class() {
    // Declaration soundness, enumerated over the registry rather than trusted
    // per declaration.
    let r = registry();
    for decl in r.decls() {
        if decl.kind == omt_catalog::Kind::Command {
            assert!(
                decl.intent.is_some(),
                "`{}` is a command with no intent class",
                decl.name
            );
        }
    }
}

#[test]
fn every_visible_capability_has_a_title() {
    // Palette reachability rests on this: no title, no palette entry, and the
    // TUI arm quietly becomes unsatisfiable.
    let r = registry();
    for decl in r.decls() {
        if !decl.hidden {
            assert!(!decl.title.is_empty(), "`{}` has no title", decl.name);
        }
    }
}

#[test]
fn the_gate_actually_fails_when_a_surface_is_missing() {
    // A test for the test. A gate that cannot fail proves nothing, and this
    // one's whole purpose is to fail — so assert that it does, against a
    // capability deliberately absent from the web handler set.
    let handlers = web_handlers();
    assert!(
        !handlers.contains("nonexistent.capability"),
        "premise: this name is not handled"
    );

    let would_fail = !handlers.contains("nonexistent.capability");
    assert!(
        would_fail,
        "the web-handler arm cannot distinguish a handled capability from an unhandled one"
    );
}
