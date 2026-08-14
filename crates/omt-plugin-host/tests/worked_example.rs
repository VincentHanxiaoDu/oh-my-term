//! The worked example, run against the code.
//!
//! `docs/architecture/11-plugins.md` §8 — the ntfy notification sink, chosen
//! there because it is the smallest plugin that exercises consent, config,
//! events, network and a contribution.
//!
//! The manifest here is not a paraphrase: it is extracted from the document
//! itself and committed beside this test. A worked example that has drifted
//! from the code it illustrates is worse than none, because it is the thing a
//! plugin author copies.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]

use omt_plugin_host::distribution::{Source, admit, admit_contribution};
use omt_plugin_host::lifecycle::{Health, Restart, Stage};
use omt_plugin_host::manifest::{Status, check_api, read};

const NTFY: &str = include_str!("fixtures/ntfy.omt-plugin.toml");

#[test]
fn the_documented_manifest_is_one_this_host_accepts() {
    // The example a plugin author copies has to parse.
    let manifest = read(NTFY);
    assert_eq!(manifest.status, Status::Valid, "{:?}", manifest.diagnostics);
    assert_eq!(manifest.id.as_deref(), Some("dev.example.ntfy"));
}

#[test]
fn the_documented_manifest_declares_a_range_this_host_satisfies() {
    let manifest = read(NTFY);
    assert!(check_api(&manifest, "1.0.0").is_ok());
    assert!(
        check_api(&manifest, "2.0.0").is_err(),
        "and it stops at the major boundary it says it does"
    );
}

#[test]
fn nothing_in_the_documented_manifest_is_an_unrecognised_key() {
    // Every table the example uses is one the host understands. If the document
    // grew a table the host does not, this is where it shows up rather than in
    // somebody's install.
    let manifest = read(NTFY);
    let unexpected: Vec<_> = manifest
        .diagnostics
        .iter()
        .map(|d| format!("{}: {}", d.code, d.key))
        .collect();
    assert!(unexpected.is_empty(), "{unexpected:?}");
}

#[test]
fn a_sink_installed_from_topic_search_is_labelled_unreviewed() {
    // §8.5's install transcript says "(unreviewed, from GitHub topic search)",
    // and that label is not cosmetic.
    assert_eq!(Source::Discovered.label(), Some("unreviewed"));
}

#[test]
fn the_sink_contributes_a_name_no_core_group_owns() {
    assert!(admit_contribution("ntfy", "ntfy.deliver", &[]).is_ok());
}

#[test]
fn editing_the_installed_sink_stops_it_at_the_next_start() {
    // The install transcript ends with a running plugin; the digest is what
    // keeps it the same plugin tomorrow.
    assert!(admit("sha256:a", "sha256:a", false, true).is_ok());
    assert!(admit("sha256:a", "sha256:edited", false, true).is_err());
}

#[test]
fn a_flaky_network_sink_degrades_and_recovers_rather_than_dying() {
    // The realistic failure for this plugin is ntfy.sh being briefly
    // unreachable. Quarantining a notification sink for that would lose the
    // notifications it exists to send.
    let mut health = Health::started(Restart::OnCrash, 5);
    health.degrade();
    assert_eq!(health.stage(), Stage::Degraded);
    health.answered();
    assert_eq!(health.stage(), Stage::Healthy);
}

#[test]
fn the_sink_cannot_answer_the_question_it_notifies_about() {
    // The point §8.5 makes explicitly: the plugin pushes the notification, and
    // the human answers. `role = "viewer"` in the manifest is what makes that
    // structural rather than a promise.
    use omt_daemon::escalation::{effective_role, permits};
    use omt_types::Role;

    // It is triggered by an event — no human behind it.
    let carried = effective_role(Role::Viewer, None);
    assert_eq!(carried, Role::Viewer);
    assert!(
        !permits(carried, Role::Operator),
        "a viewer sink cannot resolve an interaction"
    );
}
