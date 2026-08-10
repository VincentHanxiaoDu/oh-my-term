//! Named arrangements somebody starts by name.
//!
//! `docs/architecture/10-configuration.md` §9.1. A launch configuration is a
//! workspace, some sessions and a layout, saved under a name.
//!
//! The rule that matters most here is not about layout at all:
//!
//! > **An agent is spawned as a PTY child with known argv — never by typing
//! > into a shell.**
//!
//! Typing into a shell is the obvious implementation and it is wrong in a way
//! that is hard to see. The text goes through the shell's own parsing, so a
//! prompt containing a backtick, a `$(`, or a newline becomes shell execution;
//! a launch file is a file people copy from a colleague, and the prompt is the
//! part they edit. Spawning with an argument vector has no shell to confuse,
//! and the prompt is one argument however it is written.
//!
//! Everything else here is layout, which is `omt-tui`'s business — this module
//! produces the description and never the rectangles.

use serde::{Deserialize, Serialize};

/// What a pane starts as.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Start {
    /// A plain shell.
    Shell,
    /// Commands run in a shell, in order.
    Commands {
        /// What to run.
        commands: Vec<String>,
    },
    /// An agent, spawned directly.
    Agent {
        /// Which agent.
        id: String,
        /// Its arguments, as a vector.
        #[serde(default)]
        args: Vec<String>,
        /// A first prompt, passed as one argument.
        #[serde(default)]
        prompt: Option<String>,
    },
}

/// One pane in a launch configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pane {
    /// Where it starts.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Whether focus lands here.
    #[serde(default)]
    pub focused: bool,
    /// What runs in it.
    #[serde(default = "shell")]
    pub start: Start,
}

fn shell() -> Start {
    Start::Shell
}

/// A saved arrangement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Launch {
    /// What it is called.
    pub name: String,
    /// One line about it.
    #[serde(default)]
    pub description: Option<String>,
    /// The panes, in order.
    pub panes: Vec<Pane>,
    /// Environment for every pane.
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, String>,
}

/// Why a launch configuration was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    /// What is wrong.
    pub problem: String,
    /// What to do about it.
    pub help: String,
}

/// The argv omt will spawn for an agent pane.
///
/// A vector, never a string. There is no shell in this path, so a prompt
/// containing a backtick or a `$(` is a prompt, not an execution.
#[must_use]
pub fn agent_argv(id: &str, args: &[String], prompt: Option<&str>) -> Vec<String> {
    let mut argv = vec![command_for(id).to_owned()];
    argv.extend(args.iter().cloned());
    if let Some(prompt) = prompt {
        // One argument, whatever is in it. Splitting on whitespace here is the
        // same mistake as using a shell, one layer down.
        argv.push(prompt.to_owned());
    }
    argv
}

/// The program an agent id is invoked as.
fn command_for(id: &str) -> &str {
    match id {
        "claude_code" => "claude",
        "gemini_cli" => "gemini",
        "qwen_code" => "qwen",
        other => other,
    }
}

/// Check a launch configuration before anything is started.
///
/// Every problem, not the first. Somebody who wrote a file with four mistakes
/// wants to fix four, not to run it four times.
#[must_use]
pub fn check(launch: &Launch) -> Vec<Refused> {
    let mut out = Vec::new();

    if launch.name.trim().is_empty() {
        out.push(Refused {
            problem: "a launch configuration needs a name".to_owned(),
            help: "It is how it is started: `omt launch <name>`.".to_owned(),
        });
    }
    if launch.panes.is_empty() {
        out.push(Refused {
            problem: format!("`{}` has no panes", launch.name),
            help: "Add at least one pane; an arrangement of nothing is not an arrangement."
                .to_owned(),
        });
    }
    if launch.panes.iter().filter(|p| p.focused).count() > 1 {
        out.push(Refused {
            problem: "more than one pane is marked focused".to_owned(),
            // Picking one silently would make the choice depend on file order,
            // which is not something the person writing it can see.
            help: "Mark exactly one, or none and omt focuses the first.".to_owned(),
        });
    }
    for pane in &launch.panes {
        if let Start::Agent { id, .. } = &pane.start
            && id.trim().is_empty()
        {
            out.push(Refused {
                problem: "an agent pane names no agent".to_owned(),
                help: "Set `id` to the agent to start, for example `claude_code`.".to_owned(),
            });
        }
    }
    out
}

/// Which pane focus lands in.
///
/// The first marked one, or the first pane. Never nothing: a launch that opened
/// with focus nowhere would have somebody pressing keys into a window that is
/// not listening.
#[must_use]
pub fn focused(launch: &Launch) -> Option<usize> {
    launch
        .panes
        .iter()
        .position(|p| p.focused)
        .or(if launch.panes.is_empty() { None } else { Some(0) })
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn pane(start: Start) -> Pane {
        Pane {
            cwd: Some("/src/api".to_owned()),
            focused: false,
            start,
        }
    }

    fn launch(panes: Vec<Pane>) -> Launch {
        Launch {
            name: "review".to_owned(),
            description: None,
            panes,
            env: std::collections::BTreeMap::new(),
        }
    }

    #[test]
    fn an_agent_is_spawned_with_an_argument_vector() {
        let argv = agent_argv(
            "claude_code",
            &["--permission-mode".to_owned(), "plan".to_owned()],
            Some("Review the diff"),
        );
        assert_eq!(
            argv,
            ["claude", "--permission-mode", "plan", "Review the diff"]
        );
    }

    #[test]
    fn a_prompt_containing_shell_syntax_stays_one_argument() {
        // The whole reason this path has no shell in it. A launch file is
        // something people copy from a colleague, and the prompt is the part
        // they edit.
        for prompt in [
            "look at `rm -rf /`",
            "check $(whoami) please",
            "first line\nsecond line",
            "it's a \"quoted\" thing; also & this",
        ] {
            let argv = agent_argv("claude_code", &[], Some(prompt));
            assert_eq!(argv.len(), 2, "{prompt:?} was split");
            assert_eq!(argv[1], prompt);
        }
    }

    #[test]
    fn an_empty_prompt_is_absent_rather_than_an_empty_argument() {
        // An empty trailing argument is not the same as no argument, and some
        // agents treat it as a prompt of nothing rather than as no prompt.
        assert_eq!(agent_argv("claude_code", &[], None), ["claude"]);
    }

    #[test]
    fn an_unknown_agent_id_is_used_as_the_program_name() {
        // A new CLI works without a release. Guessing wrong here fails loudly
        // at spawn time, which is better than refusing something that exists.
        assert_eq!(agent_argv("newthing", &[], None), ["newthing"]);
    }

    #[test]
    fn a_configuration_with_nothing_wrong_is_accepted() {
        assert!(check(&launch(vec![pane(Start::Shell)])).is_empty());
    }

    #[test]
    fn an_arrangement_of_nothing_is_refused() {
        let found = check(&launch(Vec::new()));
        assert_eq!(found.len(), 1);
        assert!(found[0].problem.contains("no panes"));
    }

    #[test]
    fn two_focused_panes_are_refused_rather_than_resolved_silently() {
        // Picking one would make the choice depend on file order, which is not
        // something the person writing it can see.
        let mut panes = vec![pane(Start::Shell), pane(Start::Shell)];
        panes[0].focused = true;
        panes[1].focused = true;
        assert_eq!(check(&launch(panes)).len(), 1);
    }

    #[test]
    fn focus_always_lands_somewhere() {
        // A launch that opened with focus nowhere would have somebody pressing
        // keys into a window that is not listening.
        assert_eq!(focused(&launch(vec![pane(Start::Shell)])), Some(0));
        let mut panes = vec![pane(Start::Shell), pane(Start::Shell)];
        panes[1].focused = true;
        assert_eq!(focused(&launch(panes)), Some(1));
    }

    #[test]
    fn an_agent_pane_that_names_no_agent_is_refused() {
        let found = check(&launch(vec![pane(Start::Agent {
            id: String::new(),
            args: Vec::new(),
            prompt: None,
        })]));
        assert_eq!(found.len(), 1);
        assert!(found[0].help.contains("claude_code"), "{}", found[0].help);
    }

    #[test]
    fn every_problem_is_reported_at_once() {
        // Somebody who wrote a file with four mistakes wants to fix four, not
        // to run it four times.
        let mut panes = vec![
            pane(Start::Agent {
                id: String::new(),
                args: Vec::new(),
                prompt: None,
            }),
            pane(Start::Shell),
        ];
        panes[0].focused = true;
        panes[1].focused = true;
        let mut broken = launch(panes);
        broken.name = String::new();
        assert_eq!(check(&broken).len(), 3);
    }

    #[test]
    fn every_refusal_carries_help() {
        let mut broken = launch(Vec::new());
        broken.name = String::new();
        for refusal in check(&broken) {
            assert!(!refusal.help.is_empty(), "{}", refusal.problem);
        }
    }

    #[test]
    fn a_launch_round_trips_through_its_serialised_form() {
        // The file on disk and the structure omt runs must be the same thing,
        // or a config that loads is not the config that was written.
        let original = launch(vec![pane(Start::Agent {
            id: "claude_code".to_owned(),
            args: vec!["--plan".to_owned()],
            prompt: Some("go".to_owned()),
        })]);
        let json = serde_json::to_string(&original).expect("to json");
        let back: Launch = serde_json::from_str(&json).expect("from json");
        assert_eq!(back, original);
    }

    #[test]
    fn a_pane_with_no_start_is_a_shell() {
        // The commonest thing anybody wants, so it is what omitting the field
        // gives.
        let parsed: Pane = serde_json::from_str(r#"{"cwd":"/src"}"#).expect("parse");
        assert_eq!(parsed.start, Start::Shell);
    }
}
