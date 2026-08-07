//! The adapters against the CLIs that are actually installed.
//!
//! A hand-written fixture proves a table is self-consistent. Only the binary
//! proves the table is *right* — which is how the Codex adapter was found to be
//! decoding a protocol Codex does not speak, and how Gemini's hook names turned
//! out not to be Claude Code's after all.
//!
//! Each test skips when its CLI is absent, so this is useful on a developer's
//! machine and harmless on one without them.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]

use std::process::{Command, Stdio};

use omt_agent_adapters::builtin;
use omt_types::AgentKind;

fn installed(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// What a CLI's own `--help` says about itself.
fn help(program: &str) -> String {
    Command::new(program)
        .arg("--help")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

#[test]
#[ignore = "needs the agent CLIs installed"]
fn copilot_really_does_offer_the_acp_flag_this_adapter_spawns() {
    // The adapter declares native mode and spawns `copilot --acp`. If that flag
    // stopped existing, omt would declare a channel it cannot open — which is
    // exactly what the tier ladder refuses when it can see it, and cannot see
    // here.
    if !installed("copilot") {
        return;
    }
    assert!(
        help("copilot").contains("--acp"),
        "copilot no longer advertises --acp"
    );
}

#[test]
#[ignore = "needs the agent CLIs installed"]
fn gemini_offers_acp_and_still_deprecates_the_experimental_flag() {
    // The adapter spawns `--acp` rather than `--experimental-acp` because the
    // binary's own help calls the latter deprecated. If that ever reverses,
    // this says so before a user finds out.
    if !installed("gemini") {
        return;
    }
    let text = help("gemini");
    assert!(text.contains("--acp"), "gemini no longer advertises --acp");
    assert!(
        text.contains("deprecated"),
        "the experimental flag is no longer marked deprecated; check which to spawn"
    );
}

#[test]
#[ignore = "needs the agent CLIs installed"]
fn gemini_still_has_its_own_hook_vocabulary() {
    // The reason Gemini has a dedicated adapter rather than the generic ACP
    // one. Its hooks are its own — `gemini hooks migrate` exists precisely
    // because they are not Claude Code's.
    if !installed("gemini") {
        return;
    }
    let out = Command::new("gemini")
        .args(["hooks", "--help"])
        .output()
        .expect("gemini hooks --help");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("migrate"),
        "gemini no longer offers a migration from Claude Code's hooks: {text}"
    );
}

#[test]
#[ignore = "needs the agent CLIs installed"]
fn every_installed_agent_has_an_adapter() {
    // The check that catches a CLI arriving on this machine that omt would run
    // and understand nothing about.
    let set = builtin();
    for (program, kind) in [
        ("claude", AgentKind::ClaudeCode),
        ("codex", AgentKind::Codex),
        ("copilot", AgentKind::Copilot),
        ("gemini", AgentKind::GeminiCli),
        ("cursor-agent", AgentKind::Cursor),
        ("opencode", AgentKind::Opencode),
    ] {
        if installed(program) {
            assert!(
                set.get(kind).is_some(),
                "`{program}` is installed and omt has no adapter for it"
            );
        }
    }
}
