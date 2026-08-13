//! A list drawn over the screen: the palette, the agents, the workspaces.
//!
//! One component for all three, because they are the same thing — a filtered
//! list somebody picks from — and three separate ones would drift in how they
//! filter, how they wrap and what they do when nothing matches.
//!
//! `ACTIONS` existed as a table with no code that drew it. The parity gate
//! checked the table and passed, which is how a capability can be "reachable
//! from the palette" when there is no palette.

/// One row somebody can pick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    /// What it is called.
    pub label: String,
    /// What it does, or where it is.
    pub detail: String,
    /// The chord that reaches it directly, where it has one.
    pub chord: Option<String>,
}

/// A list over the screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overlay {
    /// What the list is for.
    pub title: String,
    /// Everything in it.
    pub choices: Vec<Choice>,
    /// What has been typed to narrow it.
    pub filter: String,
    /// Which row is selected, as an index into the *filtered* list.
    pub cursor: usize,
}

impl Overlay {
    /// A list, with nothing typed yet.
    #[must_use]
    pub fn new(title: &str, choices: Vec<Choice>) -> Self {
        Self {
            title: title.to_owned(),
            choices,
            filter: String::new(),
            cursor: 0,
        }
    }

    /// Replace the rows, keeping the filter somebody typed and their place in
    /// the list.
    ///
    /// A list of what agents are doing is worthless as a snapshot: it is
    /// opened *because* something is expected to change, and an agent that
    /// starts needing a human while the list is on screen is exactly the event
    /// it exists to show. Rebuilding a whole `Overlay` each frame would throw
    /// away the filter mid-word.
    pub fn set_choices(&mut self, choices: Vec<Choice>) {
        self.choices = choices;
        // The cursor is clamped rather than reset: a list that jumped back to
        // the top whenever a row appeared could not be navigated at all.
        let len = self.visible().len();
        self.cursor = self.cursor.min(len.saturating_sub(1));
    }

    /// What is showing, given what has been typed.
    ///
    /// Matched against the label *and* the detail: somebody looking for the
    /// worktree capability may well type the branch name they saw beside it.
    #[must_use]
    pub fn visible(&self) -> Vec<&Choice> {
        if self.filter.is_empty() {
            return self.choices.iter().collect();
        }
        let needle = self.filter.to_lowercase();
        self.choices
            .iter()
            .filter(|c| {
                c.label.to_lowercase().contains(&needle)
                    || c.detail.to_lowercase().contains(&needle)
            })
            .collect()
    }

    /// The row somebody is on, if any.
    #[must_use]
    pub fn selected(&self) -> Option<&Choice> {
        self.visible().get(self.cursor).copied()
    }

    /// Move the cursor, staying inside the list.
    ///
    /// Clamped rather than wrapping: a list that jumps from the bottom to the
    /// top on one keypress loses somebody's place in a way they did not ask
    /// for.
    pub fn move_by(&mut self, delta: isize) {
        let count = self.visible().len();
        if count == 0 {
            self.cursor = 0;
            return;
        }
        let wanted = self.cursor as isize + delta;
        self.cursor = wanted.clamp(0, count as isize - 1) as usize;
    }

    /// Type into the filter, and keep the cursor on something that exists.
    pub fn push(&mut self, ch: char) {
        self.filter.push(ch);
        self.cursor = 0;
    }

    /// Delete from the filter.
    pub fn backspace(&mut self) {
        self.filter.pop();
        self.cursor = 0;
    }
}

/// Draw the overlay as lines, each exactly `cols` wide.
///
/// Lines rather than escape sequences, so what it says can be tested without a
/// terminal. Where it is painted is the caller's business.
#[must_use]
pub fn render(overlay: &Overlay, cols: u16, rows: u16) -> Vec<String> {
    let width = cols as usize;
    let mut out = vec![fit(&format!(" {} ", overlay.title), width)];

    let typed = if overlay.filter.is_empty() {
        " type to filter".to_owned()
    } else {
        format!(" {}", overlay.filter)
    };
    out.push(fit(&typed, width));

    let visible = overlay.visible();
    if visible.is_empty() {
        // Said, rather than an empty box: an empty list looks like a broken
        // one, and the useful information is that the filter matched nothing.
        out.push(fit(&format!("  nothing matches {}", overlay.filter), width));
        return out;
    }

    // Two rows are the title and the filter; one more is left for a hint.
    let room = (rows as usize).saturating_sub(3).max(1);
    let first = overlay.cursor.saturating_sub(room - 1);
    for (offset, choice) in visible.iter().skip(first).take(room).enumerate() {
        let picked = first + offset == overlay.cursor;
        let chord = choice.chord.clone().unwrap_or_default();
        let left = format!("{}{}", if picked { "> " } else { "  " }, choice.label);
        let right = if chord.is_empty() {
            choice.detail.clone()
        } else {
            format!("{}  {chord}", choice.detail)
        };
        let gap = width.saturating_sub(left.chars().count() + right.chars().count() + 2);
        out.push(fit(&format!("{left}{}{right} ", " ".repeat(gap)), width));
    }
    if visible.len() > room {
        // Never silently truncated: a list showing eight of forty is one
        // somebody believes is all of them.
        out.push(fit(&format!("  {} more", visible.len() - room), width));
    }
    out
}

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

    fn choice(label: &str, detail: &str) -> Choice {
        Choice {
            label: label.to_owned(),
            detail: detail.to_owned(),
            chord: None,
        }
    }

    fn palette() -> Overlay {
        Overlay::new(
            "Commands",
            vec![
                choice("session.create", "Start a session"),
                choice("worktree.add", "Create a git worktree"),
                choice("git.status", "Branch and working tree"),
            ],
        )
    }

    #[test]
    fn everything_shows_before_anything_is_typed() {
        assert_eq!(palette().visible().len(), 3);
    }

    #[test]
    fn typing_narrows_it() {
        let mut p = palette();
        for ch in "worktree".chars() {
            p.push(ch);
        }
        assert_eq!(p.visible().len(), 1);
    }

    #[test]
    fn it_matches_what_a_row_does_and_not_only_its_name() {
        // Somebody looking for the worktree capability may well type the word
        // they saw beside it rather than its dotted name.
        let mut p = palette();
        for ch in "branch".chars() {
            p.push(ch);
        }
        assert_eq!(p.selected().map(|c| c.label.as_str()), Some("git.status"));
    }

    #[test]
    fn nothing_matching_says_so_rather_than_drawing_an_empty_box() {
        let mut p = palette();
        for ch in "zzzz".chars() {
            p.push(ch);
        }
        let drawn = render(&p, 60, 20).join("\n");
        assert!(drawn.contains("nothing matches"), "{drawn}");
    }

    #[test]
    fn the_cursor_stays_inside_the_list() {
        // Wrapping loses somebody's place in a way they did not ask for.
        let mut p = palette();
        p.move_by(-5);
        assert_eq!(p.cursor, 0);
        p.move_by(50);
        assert_eq!(p.cursor, 2);
    }

    #[test]
    fn the_cursor_goes_back_to_the_top_when_the_filter_changes() {
        // Left where it was, it points at a row that is no longer there.
        let mut p = palette();
        p.move_by(2);
        p.push('g');
        assert_eq!(p.cursor, 0);
    }

    #[test]
    fn backspace_widens_it_again() {
        let mut p = palette();
        for ch in "worktree".chars() {
            p.push(ch);
        }
        for _ in 0..8 {
            p.backspace();
        }
        assert_eq!(p.visible().len(), 3);
    }

    #[test]
    fn every_line_is_the_width_it_was_given() {
        let p = palette();
        for cols in [20u16, 40, 100] {
            for line in render(&p, cols, 20) {
                assert_eq!(line.chars().count(), cols as usize);
            }
        }
    }

    #[test]
    fn a_list_that_does_not_fit_says_how_much_it_did_not_show() {
        let many: Vec<Choice> = (0..40)
            .map(|i| choice(&format!("cap{i}"), "does a thing"))
            .collect();
        let p = Overlay::new("Commands", many);
        let drawn = render(&p, 60, 10).join("\n");
        assert!(drawn.contains("more"), "{drawn}");
    }

    #[test]
    fn the_selected_row_scrolls_into_view() {
        let many: Vec<Choice> = (0..40).map(|i| choice(&format!("cap{i}"), "x")).collect();
        let mut p = Overlay::new("Commands", many);
        p.move_by(30);
        let drawn = render(&p, 60, 10).join("\n");
        assert!(drawn.contains("cap30"), "{drawn}");
    }

    #[test]
    fn the_selected_row_is_marked() {
        let drawn = render(&palette(), 60, 20).join("\n");
        assert!(drawn.contains("> session.create"), "{drawn}");
    }
}
