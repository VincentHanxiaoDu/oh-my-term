//! The parts of the screen that are omt's rather than the program's.
//!
//! The TUI drew one thing: the pty grid, plus a single hint line. Everything
//! else omt knows — which workspace this is, what other sessions are open,
//! whether an agent is waiting — existed in the daemon and appeared nowhere. A
//! multiplexer whose screen is indistinguishable from the shell it wraps is a
//! multiplexer nobody can tell they are running.
//!
//! Rendered as strings here, apart from the terminal, so what it says can be
//! tested without a pty. What can be wrong is the *content* — a count that
//! disagrees with the list beside it, a name that overflows and pushes
//! everything off the row. Writing bytes to a screen cannot.

/// One session, as the bar shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab {
    /// What it is called.
    pub title: String,
    /// Whether it is the one being typed into.
    pub focused: bool,
    /// How many agent threads in it are waiting for a human.
    pub blocked: usize,
    /// Whether an agent is working in it.
    pub working: bool,
}

/// What the bar is told about the instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chrome {
    /// The workspace's short name.
    pub workspace: String,
    /// Its git branch, where it has one.
    pub branch: Option<String>,
    /// The sessions, in order.
    pub tabs: Vec<Tab>,
}

/// The marker a tab wears.
///
/// A tab that needs somebody outranks one that is merely busy: the point of
/// the mark is to be found, and two marks competing for attention find neither.
fn mark(tab: &Tab) -> &'static str {
    if tab.blocked > 0 {
        "!"
    } else if tab.working {
        "*"
    } else {
        " "
    }
}

/// How the tab bar reads at this width.
///
/// Truncated from the *middle* of the list rather than the end: the focused tab
/// is the one somebody is looking for, and a bar that drops it because it is
/// seventh is a bar that answers the wrong question.
#[must_use]
pub fn tab_bar(chrome: &Chrome, cols: u16) -> String {
    let width = cols as usize;
    if chrome.tabs.is_empty() {
        return fit(&format!(" {} — nothing running", chrome.workspace), width);
    }

    // The count is decided first, because the tabs have to be chosen against
    // the room that is left once it is placed — chosen first and truncated
    // afterwards, the `+N` marker is the thing that gets cut off.
    let waiting: usize = chrome.tabs.iter().map(|t| t.blocked).sum();
    let right = if waiting > 0 {
        format!("{waiting} waiting ")
    } else {
        format!(
            "{} session{} ",
            chrome.tabs.len(),
            if chrome.tabs.len() == 1 { "" } else { "s" }
        )
    };
    let room = width.saturating_sub(right.chars().count() + 3);

    let focused = chrome.tabs.iter().position(|t| t.focused).unwrap_or(0);
    let mut shown: Vec<String> = Vec::new();
    let mut used = 0usize;

    // Outward from the focused tab, so it is always on the bar.
    let mut order: Vec<usize> = vec![focused];
    for step in 1..chrome.tabs.len() {
        if focused >= step {
            order.push(focused - step);
        }
        if focused + step < chrome.tabs.len() {
            order.push(focused + step);
        }
    }
    let mut kept: Vec<(usize, String)> = Vec::new();
    for index in order {
        let Some(tab) = chrome.tabs.get(index) else {
            continue;
        };
        let name = clip(&tab.title, 14);
        let label = if tab.focused {
            format!(" {}[{name}] ", mark(tab))
        } else {
            format!(" {}{name} ", mark(tab))
        };
        if used + label.chars().count() > room && !kept.is_empty() {
            break;
        }
        used += label.chars().count();
        kept.push((index, label));
    }
    kept.sort_by_key(|(index, _)| *index);
    for (_, label) in kept {
        shown.push(label);
    }

    let more = chrome.tabs.len() - shown.len();
    let mut line = shown.concat();
    if more > 0 {
        // Said, because a bar that silently shows six of nine is a bar somebody
        // trusts to be all of them.
        line.push_str(&format!("+{more}"));
    }

    // The count sits on the bar itself, not on the row below: that row carries
    // a hint whenever there is one to give, so anything put there is invisible
    // exactly when somebody is new enough to need it. Counted from the same
    // tabs rendered to the left of it, so the two cannot disagree.
    let space = width.saturating_sub(right.chars().count());
    if line.chars().count() > space {
        line = line.chars().take(space).collect();
    }
    let pad = space.saturating_sub(line.chars().count());
    fit(&format!("{line}{}{right}", " ".repeat(pad)), width)
}

/// The status line: where you are, and what needs you.
#[must_use]
pub fn status_line(chrome: &Chrome, cols: u16) -> String {
    let waiting: usize = chrome.tabs.iter().map(|t| t.blocked).sum();
    let working = chrome.tabs.iter().filter(|t| t.working).count();

    let mut left = chrome.workspace.clone();
    if let Some(branch) = &chrome.branch {
        left.push_str(&format!("  {branch}"));
    }

    // Counted from the same tabs the bar renders, so the two cannot disagree —
    // a header counting one thing and a body counting another is the bug this
    // project keeps finding.
    let right = if waiting > 0 {
        format!("{waiting} waiting on you")
    } else if working > 0 {
        format!("{working} working")
    } else {
        format!(
            "{} session{}",
            chrome.tabs.len(),
            if chrome.tabs.len() == 1 { "" } else { "s" }
        )
    };

    let width = cols as usize;
    let gap = width.saturating_sub(left.chars().count() + right.chars().count() + 2);
    fit(&format!(" {left}{}{right} ", " ".repeat(gap)), width)
}

/// Cut a string to a width, ending in an ellipsis when it did not fit.
fn clip(text: &str, most: usize) -> String {
    if text.chars().count() <= most {
        return text.to_owned();
    }
    let kept: String = text.chars().take(most.saturating_sub(1)).collect();
    format!("{kept}…")
}

/// Pad or cut a line to exactly the width, so it paints over what was there.
fn fit(text: &str, width: usize) -> String {
    let count = text.chars().count();
    if count > width {
        return text.chars().take(width).collect();
    }
    format!("{text}{}", " ".repeat(width - count))
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "in a test, expect() is the assertion")]
mod tests {
    use super::*;

    fn tab(title: &str) -> Tab {
        Tab {
            title: title.to_owned(),
            focused: false,
            blocked: 0,
            working: false,
        }
    }

    fn chrome(tabs: Vec<Tab>) -> Chrome {
        Chrome {
            workspace: "oh-my-term".to_owned(),
            branch: Some("main".to_owned()),
            tabs,
        }
    }

    #[test]
    fn a_bar_says_what_is_open() {
        let c = chrome(vec![tab("shell"), tab("claude")]);
        let bar = tab_bar(&c, 60);
        assert!(bar.contains("shell"), "{bar}");
        assert!(bar.contains("claude"), "{bar}");
    }

    #[test]
    fn the_focused_tab_is_marked() {
        let mut c = chrome(vec![tab("shell"), tab("claude")]);
        c.tabs[1].focused = true;
        assert!(tab_bar(&c, 60).contains("[claude]"));
    }

    #[test]
    fn a_tab_that_needs_somebody_outranks_one_that_is_busy() {
        // The point of the mark is to be found. Two marks competing for
        // attention find neither.
        let mut t = tab("claude");
        t.blocked = 1;
        t.working = true;
        assert_eq!(mark(&t), "!");
    }

    #[test]
    fn the_focused_tab_survives_a_narrow_screen() {
        // Dropped because it is seventh, the bar answers the wrong question.
        let mut tabs: Vec<Tab> = (0..9).map(|i| tab(&format!("session{i}"))).collect();
        tabs[7].focused = true;
        let bar = tab_bar(&chrome(tabs), 40);
        assert!(bar.contains("[session7]"), "{bar}");
    }

    #[test]
    fn the_bar_says_how_many_sessions_there_are() {
        // On the bar, not on the row below: that row carries a hint whenever
        // there is one, so a count put there is invisible exactly when somebody
        // is new enough to need it.
        assert!(tab_bar(&chrome(vec![tab("a")]), 60).contains("1 session"));
        assert!(tab_bar(&chrome(vec![tab("a"), tab("b")]), 60).contains("2 sessions"));
    }

    #[test]
    fn the_bar_leads_with_what_is_waiting() {
        let mut c = chrome(vec![tab("a"), tab("b")]);
        c.tabs[1].blocked = 3;
        assert!(tab_bar(&c, 60).contains("3 waiting"));
    }

    #[test]
    fn a_bar_that_shows_some_says_how_many_it_did_not() {
        // Wider than the narrowest case, because the count now takes room on
        // the right and at forty columns there is space for one tab and the
        // count and nothing else.
        let tabs: Vec<Tab> = (0..12).map(|i| tab(&format!("session{i}"))).collect();
        let bar = tab_bar(&chrome(tabs), 60);
        assert!(bar.contains('+'), "{bar}");
    }

    #[test]
    fn every_line_is_exactly_the_width_it_was_given() {
        // Short, and the row keeps whatever was under it; long, and it wraps
        // and pushes the whole screen down by one.
        let c = chrome(vec![tab("shell")]);
        for cols in [20u16, 40, 80, 200] {
            assert_eq!(tab_bar(&c, cols).chars().count(), cols as usize);
            assert_eq!(status_line(&c, cols).chars().count(), cols as usize);
        }
    }

    #[test]
    fn the_status_counts_what_the_bar_shows() {
        let mut c = chrome(vec![tab("a"), tab("b")]);
        c.tabs[0].blocked = 2;
        assert!(status_line(&c, 80).contains("2 waiting on you"));
    }

    #[test]
    fn it_says_where_you_are() {
        let c = chrome(vec![tab("a")]);
        let line = status_line(&c, 80);
        assert!(line.contains("oh-my-term"), "{line}");
        assert!(line.contains("main"), "{line}");
    }

    #[test]
    fn nothing_running_says_so_rather_than_showing_an_empty_bar() {
        assert!(tab_bar(&chrome(vec![]), 60).contains("nothing running"));
    }
}
