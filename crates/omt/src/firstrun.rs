//! What omt says the first time somebody runs it.
//!
//! One screen, printed and left behind — no question, no wait. Every question
//! is a step, and a step is where people stop.

/// Say what omt found, once, on a machine that has never run it.
///
/// The trigger is the absence of a configuration directory, because that is
/// what "first run" means and it needs no state of its own. The directory is
/// created here, so a second run says nothing — including a second run that
/// changes nothing else.
pub(crate) fn first_run_summary() {
    let Some(home) = config_home() else {
        return;
    };
    if home.exists() {
        return;
    }
    // Created before printing. If creating it fails the summary is skipped
    // rather than printed on every start, which would be worse than never.
    if std::fs::create_dir_all(&home).is_err() {
        return;
    }

    let shell = std::env::var("SHELL")
        .ok()
        .and_then(|p| {
            std::path::Path::new(&p)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "your shell".to_owned());

    let agents = detected_agents();

    println!("omt {}", env!("CARGO_PKG_VERSION"));
    println!("  shell: {shell}");
    if agents.is_empty() {
        println!("  agents: none on PATH — omt runs anything, and detects these when present");
    } else {
        println!("  agents: {}", agents.join(", "));
    }
    let leader = omt_tui::leader();
    println!("  {leader} ? for every key · {leader} d to detach · `omt web` for a phone");
    println!();
}

/// Which agent CLIs are on PATH.
///
/// Named rather than counted: "3 agents detected" tells somebody nothing they
/// can act on, and the list is what makes it obvious omt already knows about
/// the one they use.
pub(crate) fn detected_agents() -> Vec<String> {
    [
        "claude",
        "codex",
        "copilot",
        "gemini",
        "cursor-agent",
        "opencode",
    ]
    .iter()
    .filter(|name| which(name))
    .map(|name| (*name).to_owned())
    .collect()
}

/// Whether a program is on PATH.
pub(crate) fn which(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
}

/// Where omt keeps its configuration.
pub(crate) fn config_home() -> Option<std::path::PathBuf> {
    std::env::var("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|_| std::env::var("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
        .ok()
        .map(|base| base.join("omt"))
}
