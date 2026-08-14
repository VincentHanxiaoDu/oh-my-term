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

#[cfg(test)]
#[allow(clippy::expect_used, reason = "in a test, expect() is the assertion")]
mod tests {
    use super::*;

    #[test]
    fn a_tab_carries_what_the_status_bar_needs_and_nothing_else() {
        // The rendering that used to live here moved to `statusbar`, which is
        // where 19 §2.1's contract is. What is left is the shape the bar is
        // told about, and its tests are the ones that assert against a width.
        let tab = Tab {
            title: "claude".to_owned(),
            focused: true,
            blocked: 1,
            working: true,
        };
        assert_eq!(crate::statusbar::glyph(&tab), "◍");
    }
}
