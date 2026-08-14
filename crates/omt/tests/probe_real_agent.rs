//! What the process probe actually sees when a real agent is in the pane.
//!
//! The screen tests answer "does the bar say claude", which is two layers
//! above the question. When it stopped saying claude the answer told nobody
//! *which* layer had failed: the pty's foreground process, the process
//! information, the fingerprint match, or the wiring above it. This asks each
//! one directly.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "in a test, expect() is the assertion"
)]

use std::time::{Duration, Instant};

use omt_pty::{Pty, PtyConfig, PtySize};

/// Where a program is on `PATH`, if it is anywhere.
fn which(program: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
}

/// Everything this process's environment would leak into a detection.
fn inherited_agent_environment() -> Vec<String> {
    const PREFIXES: [&str; 6] = [
        "CLAUDE",
        "ANTHROPIC",
        "AI_AGENT",
        "CODEX",
        "OPENCODE",
        "GEMINI",
    ];
    std::env::vars()
        .map(|(name, _)| name)
        .filter(|name| {
            let upper = name.to_uppercase();
            PREFIXES.iter().any(|p| upper.starts_with(p))
        })
        .collect()
}

#[test]
fn every_layer_of_the_probe_reports_what_it_saw() {
    let Some(claude) = which("claude") else {
        eprintln!("skipped: no claude on PATH");
        return;
    };

    let pty = Pty::spawn(&PtyConfig {
        program: claude.clone(),
        args: Vec::new(),
        cwd: Some(std::env::temp_dir()),
        size: PtySize::new(100, 30),
        env: vec![("TERM".to_owned(), "xterm-256color".to_owned())],
        env_remove: inherited_agent_environment(),
    })
    .expect("spawn the agent on a pty");

    // Long enough for the agent to be up and to be the foreground process.
    let deadline = Instant::now() + Duration::from_secs(6);
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(200));
    }

    let fd = pty.as_raw_fd();
    let pgid = omt_pty::foreground_process(fd);
    eprintln!("foreground_process -> {pgid:?}");
    let pgid = pgid.expect("the pty has a foreground process group");

    let info = omt_pty::process_info(pgid);
    eprintln!(
        "process_info -> {:?}",
        info.as_ref().map(|i| (&i.exe, i.argv.len()))
    );
    let info = info.expect("the foreground process can be described");

    // Both names, because they disagree and the disagreement is the defect.
    // A self-updating agent runs from a versioned path behind a launcher: the
    // executable's base name is `2.1.233` and the name is only in `argv[0]`.
    eprintln!("command_name -> {:?}", info.command_name());
    eprintln!("argv0_name   -> {:?}", info.argv0_name());
    let names = info.names();
    eprintln!("names        -> {names:?}");
    assert!(
        names.contains(&"claude"),
        "no name this process goes by is the agent's: exe={:?} argv={:?}",
        info.exe,
        info.argv
    );

    let env = omt_pty::read_environ(pgid);
    eprintln!("read_environ -> {:?}", env.as_ref().map(std::vec::Vec::len));

    let set = omt_agent_adapters::builtin();
    let refs: Vec<&dyn omt_agent_adapters::AgentAdapter> = set.all();
    let found = omt::probe::identify(fd, &refs);
    eprintln!("identify -> {found:?}");
    let found = found.expect("identify reports the agent the layers below found");
    assert_eq!(found.agent, omt_types::AgentKind::ClaudeCode);
}
