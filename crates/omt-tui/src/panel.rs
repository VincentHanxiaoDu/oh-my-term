//! What a list of agents says, computed once.
//!
//! Two surfaces built their own rows from the same data and drifted: one called
//! a state "blocked" and the other "waiting on you", in the same product, and a
//! person found it rather than a test. Discipline does not fix that; one
//! computation does.
//!
//! So this is the model behind every list of sessions omt draws — the panel,
//! the leader key's list, and the roster on a phone. It decides the mark, the
//! words and the order, and knows nothing about where any of them is drawn.

use omt_types::AgentState;

/// One session, as a list shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Which session, so a row can be acted on.
    pub session: String,
    /// The agent's name where omt found one, and the session's title where it
    /// did not.
    ///
    /// The agent first, because a list opened to find out which agent is
    /// waiting answers with the word in the first column — and answering
    /// "shell" there is how omt came to look like it had not noticed.
    pub label: String,
    /// Where it is: the workspace, and the branch where there is one.
    pub place: String,
    /// What it is doing, in the words both surfaces use.
    pub state: String,
    /// The mark for what it wants: filled, hollow, or a point.
    pub mark: &'static str,
    /// How much it wants a person. Lower is more.
    pub attention: u8,
    /// Whether it is on screen here.
    ///
    /// Said in the row rather than deciding whether the row exists: "it is
    /// running, you just cannot see it" is the answer somebody is looking for,
    /// and omitting the row looks like the session is gone.
    pub on_screen: bool,
}

/// What one session is, before it is worded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// Its id.
    pub session: String,
    /// Its own title, for when no agent was found.
    pub title: String,
    /// The agent omt found, where it found one.
    pub agent: Option<String>,
    /// What that agent is doing.
    pub state: AgentState,
    /// The workspace's short name.
    pub workspace: String,
    /// Its branch, where it has one.
    pub branch: Option<String>,
    /// Whether anybody has looked at it since its agent stopped working.
    pub seen: bool,
    /// Whether it is on screen.
    pub on_screen: bool,
}

/// Turn sessions into rows, ordered by what each wants.
///
/// Across the whole list rather than within a workspace: a blocked agent in the
/// third project is above an idle one in the first, because the question a list
/// answers is "who needs me" and not "what is in this directory".
#[must_use]
pub fn entries(sessions: &[Session]) -> Vec<Entry> {
    let mut rows: Vec<Entry> = sessions
        .iter()
        .map(|s| Entry {
            session: s.session.clone(),
            label: s.agent.clone().unwrap_or_else(|| s.title.clone()),
            place: match &s.branch {
                Some(branch) => format!("{} · {branch}", s.workspace),
                None => s.workspace.clone(),
            },
            // The short form: a panel row is twenty-three columns and has to
            // hold a mark, a name and a state. The long words are for lists
            // that have the width.
            state: crate::statusbar::state_word_short(&s.state, s.seen).to_owned(),
            mark: crate::statusbar::state_mark(&s.state, s.seen),
            attention: crate::statusbar::attention(&s.state, s.seen),
            on_screen: s.on_screen,
        })
        .collect();
    // Within one level of attention, by name, so the list does not shuffle
    // between glances.
    rows.sort_by(|a, b| {
        a.attention
            .cmp(&b.attention)
            .then_with(|| a.label.cmp(&b.label))
    });
    rows
}

/// How wide the panel is when it is drawn.
///
/// Fixed rather than proportional: a list of session names does not read better
/// at 40 columns and does read worse at 16.
pub const PANEL_COLS: u16 = 24;

/// Whether there is room for the panel beside `panes` panes.
///
/// Against the same minimum a split is refused for: a panel may never do what
/// splitting is not allowed to do. 19 §2.1b.
#[must_use]
pub fn fits(cols: u16, panes: usize) -> bool {
    let Some(left) = cols.checked_sub(PANEL_COLS) else {
        return false;
    };
    let panes = u16::try_from(panes).unwrap_or(u16::MAX).max(1);
    // Each pane keeps the minimum, plus a separator between them.
    left >= panes.saturating_mul(crate::layout::MIN_PANE_COLS) + panes.saturating_sub(1)
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn session(id: &str, agent: Option<&str>, state: AgentState, seen: bool) -> Session {
        Session {
            session: id.to_owned(),
            title: "shell".to_owned(),
            agent: agent.map(str::to_owned),
            state,
            workspace: "omt".to_owned(),
            branch: Some("main".to_owned()),
            seen,
            on_screen: true,
        }
    }

    fn blocked() -> AgentState {
        AgentState::Blocked {
            reason: omt_types::BlockReason::Unspecified,
            interaction: None,
        }
    }

    #[test]
    fn the_agent_is_the_label_and_the_title_is_the_fallback() {
        // A list opened to find out which agent is waiting answers with the
        // word in the first column, and answering "shell" there is how omt
        // came to look like it had not noticed.
        let rows = entries(&[
            session("a", Some("Claude Code"), AgentState::Idle, true),
            session("b", None, AgentState::Idle, true),
        ]);
        assert_eq!(rows[0].label, "Claude Code");
        assert_eq!(rows[1].label, "shell");
    }

    #[test]
    fn who_needs_me_beats_what_is_in_this_directory() {
        // Ordered across the whole list rather than within a workspace.
        let rows = entries(&[
            session("idle", Some("a"), AgentState::Idle, true),
            session(
                "working",
                Some("b"),
                AgentState::Working { detail: None },
                true,
            ),
            session("waiting", Some("c"), blocked(), true),
            session("done", Some("d"), AgentState::Idle, false),
        ]);
        let order: Vec<&str> = rows.iter().map(|r| r.session.as_str()).collect();
        assert_eq!(order, ["waiting", "done", "working", "idle"]);
    }

    #[test]
    fn the_panel_uses_the_short_form_of_the_same_words() {
        // A panel row is twenty-three columns and has to hold a mark, a name
        // and a state, so it takes the short form — and the short form is a
        // *pair* with the long one, not an independent invention. Two
        // surfaces inventing their own words is the drift this model exists
        // to prevent, so the pairs are listed here: changing one without the
        // other fails.
        for (state, seen, long, short) in [
            (blocked(), true, "waiting on you", "waiting"),
            (
                AgentState::Working { detail: None },
                true,
                "working",
                "working",
            ),
            (AgentState::Idle, false, "done", "done"),
            (AgentState::Idle, true, "idle", "idle"),
            (AgentState::Starting, true, "starting", "starting"),
            (
                AgentState::Unknown,
                true,
                "state not reported yet",
                "looking",
            ),
        ] {
            assert_eq!(
                crate::statusbar::state_word(&state, seen),
                long,
                "{state:?}"
            );
            assert_eq!(
                crate::statusbar::state_word_short(&state, seen),
                short,
                "{state:?}"
            );
            // Neither form ever says it, at any width.
            assert!(!long.contains("unknown") && !short.contains("unknown"));
            // And the short one has to actually be short enough to be worth
            // having: it is chosen for a column that cannot fit the other.
            assert!(short.chars().count() <= 8, "{short:?} is not short");
        }

        let rows = entries(&[session("a", Some("x"), blocked(), true)]);
        assert_eq!(rows[0].state, "waiting");
    }

    #[test]
    fn nothing_says_unknown() {
        let rows = entries(&[session("a", Some("x"), AgentState::Unknown, true)]);
        assert!(!rows[0].state.contains("unknown"), "{}", rows[0].state);
    }

    #[test]
    fn a_row_says_where_it_is() {
        // A roster is mostly sessions called "shell"; without this every row
        // on the screen is identical and picking one is guessing.
        let rows = entries(&[session("a", None, AgentState::Idle, true)]);
        assert!(rows[0].place.contains("omt"), "{}", rows[0].place);
        assert!(rows[0].place.contains("main"), "{}", rows[0].place);
    }

    #[test]
    fn the_order_is_stable_between_glances() {
        // Within one level of attention, by name. A list that reshuffles is
        // one somebody has to re-read every time they look at it.
        let rows = entries(&[
            session("b", Some("zeta"), AgentState::Idle, true),
            session("a", Some("alpha"), AgentState::Idle, true),
        ]);
        assert_eq!(rows[0].label, "alpha");
    }

    #[test]
    fn a_wide_terminal_has_room_and_a_narrow_one_does_not() {
        // 19 §2.1b's table, as a test: 24 columns is worth it at 170 and is
        // not at 80, and the check is the same minimum a split is refused for.
        assert!(fits(170, 1));
        assert!(fits(120, 1));
        assert!(fits(100, 1));
        assert!(!fits(60, 1), "it took columns a pane needed");
    }

    #[test]
    fn more_panes_need_more_room() {
        // Three panes and a panel is four things sharing the width.
        assert!(fits(200, 3));
        assert!(!fits(140, 3));
    }

    #[test]
    fn a_terminal_narrower_than_the_panel_is_not_arithmetic_that_wraps() {
        assert!(!fits(10, 1));
        assert!(!fits(0, 1));
    }
}

/// Draw the panel, one line per screen row.
///
/// Returns the lines to write at the left edge, already the panel's width. The
/// caller places them; this decides what they say.
///
/// Truncation is by *width*, not by length: a CJK session name is one character
/// and two columns, and cutting by length paints into the pane beside it.
#[must_use]
pub fn render(entries: &[Entry], rows: u16) -> Vec<String> {
    let inner = usize::from(PANEL_COLS) - 1;
    let mut out = Vec::new();
    out.push(fit_columns("agents", inner));

    if entries.is_empty() {
        // Said rather than left blank. An empty panel looks like a broken one,
        // and "nothing is running" is a fact worth a line.
        out.push(fit_columns("", inner));
        out.push(fit_columns("nothing running", inner));
    }

    // Two rows per entry: what it is, then where and how it is. One row cannot
    // hold a name, a place and a state in twenty-three columns without cutting
    // all three to uselessness.
    for entry in entries {
        if out.len() + 2 > usize::from(rows) {
            let hidden = entries.len() - (out.len().saturating_sub(1)) / 2;
            if hidden > 0 {
                out.push(fit_columns(&format!("{hidden} more"), inner));
            }
            break;
        }
        out.push(fit_columns(
            &format!("{} {}", entry.mark, entry.label),
            inner,
        ));
        let where_and_how = if entry.on_screen {
            format!("  {}", entry.state)
        } else {
            // Not on screen is worth saying: it is the answer to "where did it
            // go", and omitting the row entirely looks like the session ended.
            format!("  {} · elsewhere", entry.state)
        };
        out.push(fit_columns(&where_and_how, inner));
    }

    while out.len() < usize::from(rows) {
        out.push(fit_columns("", inner));
    }
    out.truncate(usize::from(rows));
    out
}

/// Pad or cut to a number of *columns*.
fn fit_columns(text: &str, columns: usize) -> String {
    let mut out = String::new();
    let mut used = 0usize;
    for ch in text.chars() {
        let w = width_of(ch);
        if used + w > columns {
            break;
        }
        out.push(ch);
        used += w;
    }
    for _ in used..columns {
        out.push(' ');
    }
    // The separator, so the panel reads as its own surface rather than as text
    // that happens to be to the left of a shell.
    out.push('\u{2502}');
    out
}

/// How many columns a character occupies.
///
/// The same question the status bar answers, and for the same reason: a CJK
/// glyph is one character and two columns, and a panel measured in characters
/// paints one column into the pane beside it.
const fn width_of(ch: char) -> usize {
    let c = ch as u32;
    // Written as comparisons rather than ranges because `contains` is not
    // const, and this is called per character of every line of every frame.
    let wide = (c >= 0x1100 && c <= 0x115F)
        || (c >= 0x2E80 && c <= 0xA4CF)
        || (c >= 0xAC00 && c <= 0xD7A3)
        || (c >= 0xF900 && c <= 0xFAFF)
        || (c >= 0xFE30 && c <= 0xFE6F)
        || (c >= 0xFF00 && c <= 0xFF60)
        || (c >= 0xFFE0 && c <= 0xFFE6)
        || (c >= 0x1F300 && c <= 0x1F64F)
        || (c >= 0x1F900 && c <= 0x1F9FF);
    if wide { 2 } else { 1 }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod render_tests {
    use super::*;

    fn entry(label: &str, state: &str, mark: &'static str) -> Entry {
        Entry {
            session: "s".to_owned(),
            label: label.to_owned(),
            place: "omt · main".to_owned(),
            state: state.to_owned(),
            mark,
            attention: 0,
            on_screen: true,
        }
    }

    #[test]
    fn every_line_is_exactly_the_panel_width() {
        // One column too many paints into the pane beside it; one too few
        // leaves a gap that reads as a rendering fault.
        let rows = render(&[entry("Claude Code", "waiting on you", "\u{25cf}")], 10);
        for line in &rows {
            let columns: usize = line.chars().map(super::width_of).sum();
            assert_eq!(columns, usize::from(PANEL_COLS), "{line:?}");
        }
    }

    #[test]
    fn a_name_too_wide_is_cut_by_columns_and_not_by_characters() {
        // A CJK glyph is one character and two columns. Cutting by length
        // overruns the panel by one column per glyph.
        let wide = entry("会话名字会话名字会话名字", "idle", "\u{25cb}");
        for line in render(&[wide], 6) {
            let columns: usize = line.chars().map(super::width_of).sum();
            assert_eq!(columns, usize::from(PANEL_COLS), "{line:?}");
        }
    }

    #[test]
    fn an_empty_panel_says_so() {
        // A blank column looks like a broken one, and "nothing is running" is
        // a fact worth a line.
        let drawn = render(&[], 8).join("\n");
        assert!(drawn.contains("nothing running"), "{drawn}");
    }

    #[test]
    fn a_list_too_long_says_how_much_it_did_not_show() {
        // Never silently truncated: a panel showing three of nine is one
        // somebody believes is all of them.
        let many: Vec<Entry> = (0..9)
            .map(|i| entry(&format!("agent{i}"), "idle", "\u{25cb}"))
            .collect();
        let drawn = render(&many, 8).join("\n");
        assert!(drawn.contains("more"), "{drawn}");
    }

    #[test]
    fn a_session_that_is_not_on_screen_says_where_it_went() {
        // Omitting the row looks like the session ended.
        let mut away = entry("Claude Code", "working", "\u{25cf}");
        away.on_screen = false;
        let drawn = render(&[away], 8).join("\n");
        assert!(drawn.contains("elsewhere"), "{drawn}");
    }

    #[test]
    fn the_panel_fills_its_height_so_nothing_shows_through() {
        // Short of the bottom, whatever the pane drew there stays visible in
        // the panel's column and the two interleave.
        assert_eq!(render(&[], 20).len(), 20);
        assert_eq!(render(&[entry("a", "idle", "\u{25cb}")], 3).len(), 3);
    }
}
