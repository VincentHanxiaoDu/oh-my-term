//! The status bar: one row, and the only permanent chrome omt has.
//!
//! [19 §2.1](../../../docs/architecture/19-onboarding.md) is a contract, not a
//! sketch: **one row, always present, never more than one in any
//! configuration**, with five cells that each have to justify their columns.
//!
//! What shipped instead was two rows — a tab bar across the top and a hint row
//! along the bottom — and the top one listed sessions under a heading nobody
//! asked for. Two rows of chrome on a 24-row terminal is 8% of the screen, and
//! the document says one for a reason: every row omt takes is a row the program
//! underneath does not get.
//!
//! The cells, left to right:
//!
//! | | |
//! |---|---|
//! | **A** | `omt`, or the instance's name. Which program am I in, and is it local? |
//! | **B** | The sessions: index, name, and the agent-state glyph |
//! | **C** | The focused pane's directory, `~`-abbreviated |
//! | **D** | The branch, and whether it is dirty |
//! | **E** | The resolved leader and `?`. The escape hatch. |
//!
//! **Overflow drops from the left of B and never truncates E.** The priority
//! is fixed — E, A, then B summarised, then C, then D — because a bar that
//! decides what to drop by measuring is a bar that shows different things on
//! two terminals a column apart.

use crate::chrome::Tab;

/// What the bar is told.
///
/// Every field is something a capability already answers, which is the point:
/// [19 §2.1](../../../docs/architecture/19-onboarding.md) declines to add an
/// `instance.status` capability, because a composed view whose only consumer
/// is one row of chrome is a catalog entry that exists to be a screenshot.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Status {
    /// Cell A: what this instance is called.
    pub instance: String,
    /// Cell A: the host, when this is a thin client onto another machine.
    ///
    /// Rendered as `omt→box`. A user who forgets they are remote will `rm` the
    /// wrong file, which is what these columns are bought with.
    pub remote_host: Option<String>,
    /// Cell B: the sessions, in order.
    pub sessions: Vec<Tab>,
    /// Cell M: whether the focused session is driven over a protocol rather
    /// than a pty. Absent for `pty`, which is the default and is unlabelled.
    pub native: bool,
    /// Cell C: the focused pane's directory.
    pub cwd: Option<String>,
    /// Cell D: the branch, where shell integration reported one.
    pub branch: Option<String>,
    /// Cell D: whether the working tree is dirty.
    pub dirty: bool,
    /// Cell E: the leader, as a person reads it.
    pub leader: String,
}

/// The glyph a session wears in cell B.
///
/// The agent state machine's own vocabulary. Idle is deliberately *nothing*:
/// most sessions are idle most of the time, and a glyph on every one of them
/// is a row of noise with the one that matters hidden in it.
#[must_use]
pub fn glyph(session: &Tab) -> &'static str {
    if session.blocked > 0 {
        "◍"
    } else if session.working {
        "●"
    } else {
        ""
    }
}

/// Cell B at full length: `1:zsh 2:claude ●`.
fn sessions_full(status: &Status) -> String {
    status
        .sessions
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let mark = glyph(s);
            if mark.is_empty() {
                format!("{}:{}", i + 1, s.title)
            } else {
                format!("{}:{} {mark}", i + 1, s.title)
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Cell B summarised: `3s ●`, or `3●` when even that is too much.
///
/// The count survives to the last, because "how many" is the half of cell B
/// that still answers something once the names are gone.
fn sessions_summary(status: &Status, tight: bool) -> String {
    let n = status.sessions.len();
    let waiting: usize = status.sessions.iter().map(|s| s.blocked).sum();
    let working = status.sessions.iter().any(|s| s.working);
    let mark = if waiting > 0 {
        "◍"
    } else if working {
        "●"
    } else {
        ""
    };
    if tight {
        format!("{n}{mark}")
    } else if mark.is_empty() {
        format!("{n}s")
    } else {
        format!("{n}s {mark}")
    }
}

/// `~`-abbreviated and middle-elided.
fn short_cwd(cwd: &str, most: usize) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let shown = if !home.is_empty() && cwd.starts_with(&home) {
        format!("~{}", &cwd[home.len()..])
    } else {
        cwd.to_owned()
    };
    if shown.chars().count() <= most || most < 4 {
        return shown;
    }
    // From the middle: the root says which machine and the tail says which
    // directory, and cutting either end throws away one of those.
    let keep = most - 1;
    let head: String = shown.chars().take(keep / 2).collect();
    let tail: String = shown
        .chars()
        .skip(shown.chars().count() - (keep - keep / 2))
        .collect();
    format!("{head}…{tail}")
}

/// Cell A.
fn instance_cell(status: &Status) -> String {
    match &status.remote_host {
        Some(host) => format!("{}→{host}", status.instance),
        None => status.instance.clone(),
    }
}

/// Cell E, which is never dropped and never truncated.
fn help_cell(status: &Status, tight: bool) -> String {
    if tight {
        format!("{}?", status.leader)
    } else {
        format!("{} ? help", status.leader)
    }
}

/// Render the bar for a width.
///
/// Never longer than `cols`, and never shorter either: it is padded, because
/// the row is repainted in place and a shorter bar drawn over a longer one
/// leaves the tail of the old one on screen.
#[must_use]
pub fn render(status: &Status, cols: u16) -> String {
    let width = cols as usize;
    if width == 0 {
        return String::new();
    }

    // Tried longest-first. Each candidate is the same bar with one more thing
    // given up, in the order 19 §2.1 fixes: E and A survive everything, then B
    // is summarised, then C goes, then D.
    let native = if status.native { "⟨native⟩" } else { "" };
    let branch = status.branch.as_ref().map(|b| {
        if status.dirty {
            format!("{b}*")
        } else {
            b.clone()
        }
    });

    for stage in 0..6u8 {
        let mut parts: Vec<String> = vec![instance_cell(status)];

        let b = match stage {
            0 | 1 => sessions_full(status),
            2..=4 => sessions_summary(status, false),
            _ => sessions_summary(status, true),
        };
        if !b.is_empty() {
            parts.push(b);
        }
        if !native.is_empty() {
            // Never dropped. D8 requires the mode to be visible on every
            // surface, and a summary that loses it is a surface that stopped
            // saying which product the user is talking to.
            parts.push(native.to_owned());
        }
        if stage <= 2
            && let Some(cwd) = &status.cwd
        {
            parts.push(short_cwd(cwd, 24));
        }
        if stage <= 1
            && let Some(branch) = &branch
        {
            parts.push(branch.clone());
        }
        parts.push(help_cell(status, stage >= 5));

        let line = parts.join(" ");
        if line.chars().count() <= width {
            return pad(&line, width);
        }
    }

    // Narrower than the escape hatch itself. It is the one thing that must
    // survive, so what is left is it, cut to fit rather than dropped.
    let last = help_cell(status, true);
    pad(&last.chars().take(width).collect::<String>(), width)
}

fn pad(line: &str, width: usize) -> String {
    let len = line.chars().count();
    if len >= width {
        return line.chars().take(width).collect();
    }
    let mut out = line.to_owned();
    for _ in len..width {
        out.push(' ');
    }
    out
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn tab(title: &str, blocked: usize, working: bool) -> Tab {
        Tab {
            title: title.to_owned(),
            focused: false,
            blocked,
            working,
        }
    }

    fn status() -> Status {
        Status {
            instance: "omt".to_owned(),
            remote_host: None,
            sessions: vec![
                tab("zsh", 0, false),
                tab("claude", 0, true),
                tab("build", 0, false),
            ],
            native: false,
            cwd: Some("/code/app".to_owned()),
            branch: Some("main".to_owned()),
            dirty: true,
            leader: "^B".to_owned(),
        }
    }

    #[test]
    fn a_wide_terminal_gets_every_cell() {
        let bar = render(&status(), 120);
        for expected in [
            "omt",
            "1:zsh",
            "2:claude",
            "●",
            "/code/app",
            "main*",
            "^B ? help",
        ] {
            assert!(bar.contains(expected), "`{expected}` missing from {bar:?}");
        }
    }

    #[test]
    fn the_escape_hatch_survives_every_width() {
        // 19 §2.1: it is the one thing a user who learns nothing else about
        // omt has to be able to find. Dropping it at any width is the failure
        // this whole priority list exists to prevent.
        for cols in 20..=200u16 {
            let bar = render(&status(), cols);
            assert!(
                bar.contains('?'),
                "no escape hatch at {cols} columns: {bar:?}"
            );
        }
    }

    #[test]
    fn the_bar_is_exactly_the_width_it_was_given_at_every_width() {
        // Shorter, and the tail of the previous paint stays on screen; longer,
        // and it wraps, which scrolls the terminal by one row and pushes the
        // program's own last line out of view.
        for cols in 1..=200u16 {
            let bar = render(&status(), cols);
            assert_eq!(
                bar.chars().count(),
                cols as usize,
                "wrong width at {cols}: {bar:?}"
            );
        }
    }

    #[test]
    fn it_degrades_in_the_documented_order() {
        // E, A, B-summary, C, D. Checked by walking the widths downward and
        // asserting nothing comes back once it has gone — a bar that drops the
        // branch at 70 columns and shows it again at 60 is one nobody can read
        // twice the same way.
        let s = status();
        let mut seen_without = std::collections::HashSet::new();
        for cols in (20..=140u16).rev() {
            let bar = render(&s, cols);
            for (name, needle) in [
                ("branch", "main*"),
                ("cwd", "/code/app"),
                ("names", "1:zsh"),
            ] {
                if bar.contains(needle) {
                    assert!(
                        !seen_without.contains(name),
                        "`{name}` came back at {cols} columns after being dropped: {bar:?}"
                    );
                } else {
                    seen_without.insert(name);
                }
            }
        }
    }

    #[test]
    fn a_waiting_session_is_marked_and_an_idle_one_is_not() {
        // Most sessions are idle most of the time. A glyph on every one is a
        // row of noise with the one that matters hidden inside it.
        assert_eq!(glyph(&tab("zsh", 0, false)), "");
        assert_eq!(glyph(&tab("claude", 0, true)), "●");
        assert_eq!(glyph(&tab("claude", 1, true)), "◍");
    }

    #[test]
    fn waiting_outranks_working_in_the_summary() {
        // The summary has room for one mark, and "something needs you" is the
        // one worth having.
        let s = Status {
            sessions: vec![tab("a", 0, true), tab("b", 2, false)],
            ..status()
        };
        assert!(sessions_summary(&s, false).contains('◍'));
    }

    #[test]
    fn a_thin_client_says_which_machine_it_is_driving() {
        // A user who forgets they are remote will `rm` the wrong file.
        let s = Status {
            remote_host: Some("box".to_owned()),
            ..status()
        };
        assert!(render(&s, 120).contains("omt→box"));
    }

    #[test]
    fn a_native_session_is_labelled_and_survives_the_summary() {
        // D8: the mode must be visible on every surface, and it does not
        // participate in the drop order — if B is summarised, the summary
        // still carries it.
        let s = Status {
            native: true,
            ..status()
        };
        for cols in 30..=120u16 {
            assert!(
                render(&s, cols).contains("⟨native⟩"),
                "the mode vanished at {cols} columns"
            );
        }
    }

    #[test]
    fn a_directory_is_cut_from_the_middle() {
        // The root says which machine and the tail says which directory.
        // Cutting either end throws one of those away.
        let short = short_cwd("/a/very/long/path/to/somewhere/deep", 20);
        assert!(short.chars().count() <= 20, "{short}");
        assert!(short.starts_with("/a"), "{short}");
        assert!(short.ends_with("deep"), "{short}");
    }

    #[test]
    fn a_home_directory_is_abbreviated() {
        // SAFETY: single-threaded at this point in the test.
        #[allow(unsafe_code, reason = "set_var is unsafe in edition 2024")]
        unsafe {
            std::env::set_var("HOME", "/Users/someone");
        }
        assert_eq!(short_cwd("/Users/someone/code", 40), "~/code");
    }

    #[test]
    fn an_instance_with_nothing_running_still_renders() {
        let s = Status {
            sessions: Vec::new(),
            cwd: None,
            branch: None,
            ..status()
        };
        let bar = render(&s, 40);
        assert_eq!(bar.chars().count(), 40);
        assert!(bar.contains('?'), "{bar:?}");
    }
}

/// What to call an agent's state to somebody looking at a list.
///
/// Takes whether the pane has been looked at since, because that is not
/// something the agent knows and not something the wire should carry: an
/// agent is idle either way, and whether *you* have seen it is a fact about
/// you.
///
/// The distinction it buys is the one this product exists for. An agent that
/// finished while its pane was off screen is not the same as one sitting at a
/// prompt you have been watching — the first is a thing to go and look at, and
/// collapsing them into "idle" is how somebody misses that the thing they were
/// waiting for is done.
#[must_use]
pub fn state_word(state: &omt_types::AgentState, seen: bool) -> &'static str {
    match state {
        omt_types::AgentState::Blocked { .. } => "waiting on you",
        omt_types::AgentState::Working { .. } => "working",
        omt_types::AgentState::Starting => "starting",
        omt_types::AgentState::Exited { .. } => "exited",
        omt_types::AgentState::Idle if !seen => "done",
        omt_types::AgentState::Idle => "idle",
        // Never "unknown". The word describes omt's knowledge, not the agent,
        // and beside an agent omt has just found it reads as "omt has no idea
        // what this is" — which is how it was read, twice, by the person it
        // was shown to.
        //
        // It is also the honest answer for the first two seconds of every
        // agent's life: the floor needs two seconds of quiet before it will
        // say idle, and a process table cannot report a lifecycle at all.
        omt_types::AgentState::Unknown => "state not reported yet",
    }
}

/// The mark an agent wears in a list.
///
/// Filled for something that wants attention or is moving, hollow for
/// something at rest that has been seen, and a dim point for a state omt
/// cannot report. Shape rather than colour alone, because a colour is the
/// first thing a terminal theme takes away.
#[must_use]
pub fn state_mark(state: &omt_types::AgentState, seen: bool) -> &'static str {
    match state {
        omt_types::AgentState::Blocked { .. } | omt_types::AgentState::Working { .. } => "\u{25cf}",
        omt_types::AgentState::Idle if !seen => "\u{25cf}",
        omt_types::AgentState::Idle | omt_types::AgentState::Starting => "\u{25cb}",
        omt_types::AgentState::Exited { .. } | omt_types::AgentState::Unknown => "\u{b7}",
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod state_word_tests {
    use super::{state_mark, state_word};
    use omt_types::AgentState;

    #[test]
    fn finished_and_unseen_is_not_idle() {
        // The state this product exists to surface. An agent that finished
        // while nobody was looking is a thing to go and look at.
        assert_eq!(state_word(&AgentState::Idle, false), "done");
        assert_eq!(state_word(&AgentState::Idle, true), "idle");
    }

    #[test]
    fn done_and_waiting_are_both_marked_rather_than_only_coloured() {
        // A colour is the first thing a terminal theme takes away, and a
        // person reading a list in a light theme they did not choose still has
        // to be able to see which row wants them.
        assert_eq!(state_mark(&AgentState::Idle, false), "\u{25cf}");
        assert_eq!(state_mark(&AgentState::Idle, true), "\u{25cb}");
        assert_ne!(
            state_mark(&AgentState::Idle, false),
            state_mark(&AgentState::Idle, true)
        );
    }

    #[test]
    fn nothing_ever_says_unknown() {
        // It describes omt's knowledge rather than the agent, and beside an
        // agent omt has just found it reads as "omt has no idea what this is".
        for state in [
            AgentState::Unknown,
            AgentState::Starting,
            AgentState::Idle,
            AgentState::Working { detail: None },
        ] {
            for seen in [true, false] {
                let said = state_word(&state, seen);
                assert!(!said.contains("unknown"), "{state:?} says {said:?}");
            }
        }
    }

    #[test]
    fn being_waited_on_outranks_having_been_seen() {
        // Looking at a pane does not answer the question in it.
        assert_eq!(
            state_word(
                &AgentState::Blocked {
                    reason: omt_types::BlockReason::Unspecified,
                    interaction: None
                },
                true
            ),
            "waiting on you"
        );
    }
}

/// How much a row wants a person, lowest first.
///
/// The order is a product judgment and not an implementation detail:
///
/// 1. waiting on you — it cannot continue without you
/// 2. **done and unseen** — it finished while you were elsewhere
/// 3. working — it is busy and needs nothing
/// 4. idle and seen — you have looked at it
/// 5. anything omt cannot describe
///
/// Done outranks working, which reads backwards until you ask what each row
/// wants. A session still working wants nothing from anybody; a finished one
/// is the whole reason somebody opened the list. Sorting by "is it moving"
/// puts the rows that need nothing at the top.
#[must_use]
pub fn attention(state: &omt_types::AgentState, seen: bool) -> u8 {
    match state {
        omt_types::AgentState::Blocked { .. } => 0,
        omt_types::AgentState::Idle if !seen => 1,
        omt_types::AgentState::Working { .. } | omt_types::AgentState::Starting => 2,
        omt_types::AgentState::Idle => 3,
        omt_types::AgentState::Exited { .. } | omt_types::AgentState::Unknown => 4,
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod attention_tests {
    use super::attention;
    use omt_types::AgentState;

    fn blocked() -> AgentState {
        AgentState::Blocked {
            reason: omt_types::BlockReason::Unspecified,
            interaction: None,
        }
    }

    #[test]
    fn a_finished_result_outranks_something_still_running() {
        // A session still working wants nothing from anybody. A finished one
        // is the whole reason somebody opened the list.
        assert!(
            attention(&AgentState::Idle, false)
                < attention(&AgentState::Working { detail: None }, true)
        );
    }

    #[test]
    fn what_cannot_continue_without_you_outranks_everything() {
        for other in [
            AgentState::Working { detail: None },
            AgentState::Idle,
            AgentState::Starting,
            AgentState::Unknown,
        ] {
            assert!(
                attention(&blocked(), true) < attention(&other, true),
                "{other:?}"
            );
        }
        assert!(attention(&blocked(), true) < attention(&AgentState::Idle, false));
    }

    #[test]
    fn looking_at_it_moves_it_down() {
        // The only thing that changes here is whether somebody looked, and it
        // is the difference between the second row and the fourth.
        assert!(attention(&AgentState::Idle, false) < attention(&AgentState::Idle, true));
    }

    #[test]
    fn the_two_surfaces_order_by_the_same_rule() {
        // The web client's `attention` has the same five ranks in the same
        // order. This is the half that can be checked from Rust; the other
        // half is a unit test over there with the same names in it.
        assert_eq!(attention(&blocked(), true), 0);
        assert_eq!(attention(&AgentState::Idle, false), 1);
        assert_eq!(attention(&AgentState::Working { detail: None }, true), 2);
        assert_eq!(attention(&AgentState::Idle, true), 3);
        assert_eq!(attention(&AgentState::Unknown, true), 4);
    }
}
