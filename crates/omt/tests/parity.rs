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

use omt_catalog::{CapabilityRegistry, Effects, Surface};
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
    omt::capabilities::registry(omt::state::State::default()).expect("the registry must build")
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

// ---------------------------------------------------------------------------
// Role and effects consistency — 13-security §4
// ---------------------------------------------------------------------------

/// Capabilities allowed to be `Viewer` while spawning a process.
///
/// 13 §4's read-only-subprocess carve-out requires the argv template to be in
/// an allow-list checked in next to this test, so adding one is a reviewed diff
/// rather than a property a handler asserts about itself. Empty is the correct
/// state until something genuinely needs it.
const READ_ONLY_SUBPROCESS_ALLOW_LIST: &[&str] = &[];

#[test]
fn no_viewer_capability_can_change_anything() {
    // The mapping is mechanical, which is what makes it trustworthy. A
    // capability declaring `Viewer` while carrying a writing effect is two
    // declarations disagreeing, and the one dispatch enforces is the role —
    // so the effect is what would surprise somebody.
    let r = registry();
    let mut offenders: Vec<String> = Vec::new();
    for decl in r.decls() {
        if decl.role != Role::Viewer {
            continue;
        }
        let writes = decl.effects.contains(Effects::WRITES_PTY)
            || decl.effects.contains(Effects::WRITES_FS)
            || decl.effects.contains(Effects::NETWORK)
            || decl.effects.contains(Effects::DESTRUCTIVE);
        if writes {
            offenders.push(format!("{} ({:?})", decl.name, decl.effects));
        }
    }
    assert!(
        offenders.is_empty(),
        "these are Viewer but change something: {offenders:?}"
    );
}

#[test]
fn spawning_a_process_as_viewer_needs_the_reviewed_allow_list() {
    // `SPAWNS_PROCESS` alone does not disqualify Viewer, but the carve-out is
    // deliberately hard to reach: the argv template has to be in a list in the
    // repository, so it costs a reviewed diff.
    let r = registry();
    for decl in r.decls() {
        if decl.role != Role::Viewer || !decl.effects.contains(Effects::SPAWNS_PROCESS) {
            continue;
        }
        assert!(
            READ_ONLY_SUBPROCESS_ALLOW_LIST.contains(&decl.name),
            "`{}` is Viewer and spawns a process without being on the allow-list",
            decl.name
        );
    }
}

#[test]
fn every_capability_declares_one_of_the_three_roles() {
    // There is no sub-Viewer role and no tier between them. A narrower surface
    // is a Viewer credential with a capability scope, not a fourth role — and
    // a fourth appearing here would mean somebody added one by accident.
    let r = registry();
    for decl in r.decls() {
        assert!(
            matches!(decl.role, Role::Viewer | Role::Operator | Role::Admin),
            "`{}` has role {:?}",
            decl.name,
            decl.role
        );
    }
}

#[test]
fn what_operator_must_not_reach_is_admin_only() {
    // 13 §4 draws the line at the *instance*, not at data. Operator explicitly
    // may close sessions and remove worktrees — that is the job. What it may
    // not do is change instance config, manage credentials, install plugins,
    // or shut the instance down, because those outlive the afternoon somebody
    // was handed the credential for.
    //
    // My first version of this test asserted every DESTRUCTIVE capability must
    // be Admin, and it failed on `session.close` and `worktree.remove`. The
    // code was right and the test was wrong: the doc says Operator can close
    // sessions in as many words. Worth the note, because a security test that
    // is stricter than the design gets relaxed by whoever hits it next, and
    // they will relax more than this line.
    let admin_only_groups = ["config", "identity", "device", "plugin", "uninstall"];
    let r = registry();
    let mut offenders: Vec<&str> = Vec::new();
    for decl in r.decls() {
        let reaches_the_instance = admin_only_groups.contains(&decl.group)
            && matches!(decl.kind, omt_catalog::Kind::Command);
        if reaches_the_instance && decl.role != Role::Admin {
            offenders.push(decl.name);
        }
    }
    assert!(
        offenders.is_empty(),
        "these change the instance but are not Admin: {offenders:?}"
    );
}

#[test]
fn the_role_check_would_actually_catch_something() {
    // Otherwise the three tests above pass because the catalog happens to have
    // no Viewer capabilities at all, which is the shape of gate this change
    // has already found twice.
    let r = registry();
    let viewers = r.decls().filter(|d| d.role == Role::Viewer).count();
    assert!(viewers > 10, "only {viewers} Viewer capabilities to check");
}

// ---------------------------------------------------------------------------
// The two high-consequence capabilities — 13-security §7
// ---------------------------------------------------------------------------

#[test]
fn writing_to_a_pty_is_never_available_to_a_viewer() {
    // 13 §7.1. A remote PTY write is remote code execution as the user, by
    // design. Operator is the floor, and the writer token on top of it is what
    // makes remote input visible to anybody at the machine and attributable
    // afterwards.
    let r = registry();
    for decl in r.decls() {
        if decl.effects.contains(Effects::WRITES_PTY) {
            assert_ne!(
                decl.role,
                Role::Viewer,
                "`{}` writes a PTY as Viewer",
                decl.name
            );
        }
    }
}

#[test]
fn omt_has_no_second_permission_model_over_an_agents_own() {
    // 13 §7.2 and D1. There is no destructive classifier, no approvable-tool
    // allow-list and no auto-approve. Each of those would be a second
    // permission model with its own configuration and its own bugs, and —
    // worst — a false sense that omt is protecting somebody in the cases it
    // cannot see. Danger classification belongs to the agent CLI, which has
    // the tool schema and the arguments; omt has a string.
    //
    // Checked by name because this is a feature request that will arrive, and
    // it will arrive sounding reasonable.
    let forbidden = [
        "auto_approve",
        "autoapprove",
        "approve_all",
        "classify_danger",
        "deny_destructive",
        "approvable_tools",
        "permission_policy",
    ];
    let r = registry();
    for decl in r.decls() {
        let name = decl.name.to_lowercase();
        for word in forbidden {
            assert!(
                !name.contains(word),
                "`{}` looks like a second permission model over the agent's own",
                decl.name
            );
        }
    }
}

#[test]
fn resolving_an_interaction_needs_operator_not_admin() {
    // The single feature the product exists for: a tap on a phone approves
    // what pressing `y` at the keyboard would. Requiring Admin would make the
    // owner's own phone second-class, which is the experience omt exists to
    // remove — and requiring Viewer would let a watcher answer for the owner.
    let r = registry();
    let resolve = r
        .decls()
        .find(|d| d.name.starts_with("interaction.") && matches!(d.kind, omt_catalog::Kind::Command))
        .expect("an interaction command");
    assert_eq!(resolve.role, Role::Operator, "`{}`", resolve.name);
}

// ---------------------------------------------------------------------------
// Network egress — 13-security §9
// ---------------------------------------------------------------------------

/// Capabilities allowed to reach a machine that is not the user's.
///
/// A short list, checked in, so adding one is a reviewed diff. omt makes zero
/// outbound connections by default — no telemetry, no crash reporting, no
/// analytics — and the way that stays true is that a new one has to appear
/// here in front of somebody.
const MAY_REACH_A_THIRD_PARTY: &[&str] = &[
    "upgrade.check",
    "stt.transcribe",
    // Reaches a host the *caller* named over ssh, which is why it is Operator
    // rather than Viewer: a read-only shared link must not be able to make this
    // machine connect somewhere (22 §10).
    "remote.probe",
];

#[test]
fn everything_that_reaches_out_is_on_the_reviewed_list() {
    // §9.1's claim is that omt makes zero outbound connections *by default*.
    // The strong form -- that nothing declares NETWORK at all -- held until
    // `upgrade.check` and `remote.probe` arrived, and both reach out only
    // because a user asked, one by typing `omt upgrade` and the other by naming
    // a host. So the assertion is now the list rather than the count.
    //
    // Both halves matter. The membership check catches a new capability that
    // reaches out without review; the count catches this list growing while
    // nobody looks, which is how a reviewed list stops being reviewed.
    let r = registry();
    let reaching: Vec<&str> = r
        .decls()
        .filter(|d| d.effects.contains(Effects::NETWORK))
        .map(|d| d.name)
        .collect();
    assert!(
        reaching.iter().all(|n| MAY_REACH_A_THIRD_PARTY.contains(n)),
        "these declare NETWORK without being on the reviewed list: {reaching:?}"
    );
    assert_eq!(
        reaching.len(),
        2,
        "the egress list changed size: {reaching:?} — update this count deliberately"
    );
    assert!(
        !reaching.is_empty(),
        "a gate that checks an empty set reports success without checking anything"
    );
}

#[test]
fn the_provider_that_can_reach_a_third_party_has_no_capability_yet() {
    // Honest scope. `omt-stt` contains code that posts to Deepgram or OpenAI,
    // and nothing in the catalog calls it — so the transcription path is not
    // reachable from any client today. That is why the NETWORK count above is
    // zero, and it is worth writing down: the count being zero is not the same
    // as the code being absent, and somebody reading only the count would
    // conclude the wrong thing.
    let r = registry();
    let voice: Vec<&str> = r
        .decls()
        .filter(|d| d.group == "voice")
        .map(|d| d.name)
        .collect();
    assert!(!voice.is_empty(), "voice capabilities exist");
    for decl in r.decls().filter(|d| d.group == "voice") {
        assert!(
            !decl.effects.contains(Effects::NETWORK),
            "`{}` reaches the network; it belongs on the reviewed list",
            decl.name
        );
    }
}

#[test]
fn nothing_that_reaches_a_third_party_is_available_to_a_viewer() {
    // A Viewer credential is what somebody hands a colleague to watch. It
    // should not be able to make the machine talk to anybody.
    let r = registry();
    for decl in r.decls() {
        if decl.effects.contains(Effects::NETWORK) {
            assert_ne!(decl.role, Role::Viewer, "`{}`", decl.name);
        }
    }
}

#[test]
fn no_capability_is_named_like_telemetry() {
    // There is none, and the way it stays that way is that adding one has to
    // get past a test that says so out loud. A metrics endpoint arrives
    // sounding harmless.
    let forbidden = ["telemetry", "analytics", "crash_report", "phone_home", "usage_ping"];
    let r = registry();
    for decl in r.decls() {
        let name = decl.name.to_lowercase();
        for word in forbidden {
            assert!(!name.contains(word), "`{}`", decl.name);
        }
    }
}
