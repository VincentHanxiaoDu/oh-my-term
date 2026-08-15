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
    /// A mark for what this row wants, where the row has a state.
    ///
    /// Shape rather than colour: a colour is the first thing a terminal theme
    /// takes away, and somebody reading a list in a theme they did not choose
    /// still has to be able to see which row wants them. A filled mark is
    /// something to attend to; a hollow one is at rest; a point is a state omt
    /// cannot report.
    pub mark: Option<&'static str>,
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
    // A bounded panel rather than the whole width. Full width put the label at
    // one edge of a 170-column terminal and its detail at the other, with a
    // hundred columns of nothing between them — the eye cannot join those, and
    // what is left does not read as a list. It also made a list drawn over an
    // agent's screen look like part of the agent's screen, because nothing
    // said where one stopped and the other began.
    let inner = width.saturating_sub(6).min(MEASURE);
    let margin = (width.saturating_sub(inner + 2)) / 2;
    let pad = " ".repeat(margin);

    let visible = overlay.visible();
    // The count in the title, because "how many are there" is the question a
    // list is opened to answer and counting rows is not an answer.
    let heading = if visible.len() == overlay.choices.len() {
        format!("{} ({})", overlay.title, overlay.choices.len())
    } else {
        format!(
            "{} ({} of {})",
            overlay.title,
            visible.len(),
            overlay.choices.len()
        )
    };

    let mut out = vec![format!(
        "{pad}\u{250c}\u{2500}{}\u{2500}{}\u{2510}",
        heading,
        "\u{2500}".repeat(inner.saturating_sub(heading.chars().count() + 2))
    )];

    let typed = if overlay.filter.is_empty() {
        "type to filter".to_owned()
    } else {
        overlay.filter.clone()
    };
    out.push(boxed(&pad, &typed, inner));

    if visible.is_empty() {
        // Said, rather than an empty box: an empty list looks like a broken
        // one, and the useful information is that the filter matched nothing.
        out.push(boxed(
            &pad,
            &format!("nothing matches {}", overlay.filter),
            inner,
        ));
        out.push(format!("{pad}\u{2514}{}\u{2518}", "\u{2500}".repeat(inner)));
        return out;
    }

    // A column for the marks, only when something has one. The palette is a
    // list of capabilities and none of them has a state, so reserving the
    // column there would indent every row for something that never appears.
    let marked = visible.iter().any(|choice| choice.mark.is_some());

    // The border rows, the filter row and a possible "more" row.
    let room = (rows as usize).saturating_sub(5).max(1);
    let first = overlay.cursor.saturating_sub(room - 1);
    for (offset, choice) in visible.iter().skip(first).take(room).enumerate() {
        let picked = first + offset == overlay.cursor;
        let chord = choice.chord.clone().unwrap_or_default();
        // Label, then detail, then the chord at the right edge. The detail
        // follows the label rather than being pushed away from it: they are
        // one row about one thing.
        let mut line = format!(
            "{}{}{}",
            if picked { "\u{276f} " } else { "  " },
            if marked {
                format!("{} ", choice.mark.unwrap_or(" "))
            } else {
                String::new()
            },
            choice.label
        );
        if !choice.detail.is_empty() {
            line.push_str("   ");
            line.push_str(&choice.detail);
        }
        if !chord.is_empty() {
            let room_left = inner.saturating_sub(line.chars().count() + chord.chars().count() + 1);
            line.push_str(&" ".repeat(room_left));
            line.push_str(&chord);
        }
        out.push(boxed(&pad, &line, inner));
    }
    if visible.len() > room {
        // Never silently truncated: a list showing eight of forty is one
        // somebody believes is all of them.
        out.push(boxed(
            &pad,
            &format!("{} more", visible.len() - room),
            inner,
        ));
    }
    out.push(format!("{pad}\u{2514}{}\u{2518}", "\u{2500}".repeat(inner)));
    out
}

/// The widest a list is allowed to get.
///
/// Wide enough for a session name, where it is and what it is doing; narrow
/// enough that the three stay in one glance.
const MEASURE: usize = 88;

/// One row inside the panel's borders.
fn boxed(pad: &str, text: &str, inner: usize) -> String {
    format!("{pad}\u{2502}{}\u{2502}", fit(&format!(" {text}"), inner))
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
            mark: None,
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
    fn no_line_is_wider_than_the_terminal() {
        // It used to be exactly the width, because the panel was the whole
        // screen. That put a label at one edge of a 170-column terminal and
        // its detail at the other — the eye cannot join those — so the panel
        // is bounded and centred now, and the invariant that is left is the
        // one that matters: nothing overruns the screen.
        let p = palette();
        for cols in [20u16, 40, 100, 200] {
            for line in render(&p, cols, 20) {
                assert!(
                    line.chars().count() <= cols as usize,
                    "{} columns of line in a {cols}-column terminal: {line:?}",
                    line.chars().count()
                );
            }
        }
    }

    #[test]
    fn the_panel_is_bounded_so_a_list_reads_as_one_thing() {
        // On a wide terminal it stops well short of the edges, and it has a
        // border — without one, a list drawn over an agent's screen looks like
        // part of the agent's screen.
        let drawn = render(&palette(), 200, 20);
        // The panel itself, not the line: the line carries the left margin
        // that centres it.
        let top = drawn.first().expect("a top border");
        // By character, not by byte: the borders are multi-byte and slicing a
        // string by a byte index found with `find` lands inside one.
        let chars: Vec<char> = top.chars().collect();
        let left = chars
            .iter()
            .position(|c| *c == '\u{250c}')
            .expect("a left corner");
        let right = chars
            .iter()
            .rposition(|c| *c == '\u{2510}')
            .expect("a right corner");
        let panel = right - left + 1;
        assert!(panel < 100, "the panel spread to {panel} columns");
        assert!(left > 10, "it is not centred: {left} columns of margin");
        assert!(
            drawn.first().is_some_and(|l| l.contains('\u{250c}')),
            "no top border"
        );
        assert!(
            drawn.last().is_some_and(|l| l.contains('\u{2514}')),
            "no bottom border"
        );
    }

    #[test]
    fn the_title_says_how_many_there_are() {
        // "How many are there" is the question a list is opened to answer, and
        // counting rows is not an answer.
        let drawn = render(&palette(), 100, 20).join("\n");
        assert!(drawn.contains("(3)"), "{drawn}");
    }

    #[test]
    fn filtering_says_how_many_of_how_many() {
        let mut p = palette();
        p.push('s');
        let drawn = render(&p, 100, 20).join("\n");
        assert!(drawn.contains(" of 3"), "{drawn}");
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
        assert!(drawn.contains("\u{276f} session.create"), "{drawn}");
    }

    #[test]
    fn a_row_keeps_its_detail_next_to_its_label() {
        // Not pushed to the far edge. They are one row about one thing, and
        // the whole reason the panel is bounded is that separating them made
        // the list unreadable on a wide screen.
        let drawn = render(&palette(), 200, 20).join("\n");
        let row = drawn
            .lines()
            .find(|l| l.contains("session.create"))
            .expect("the row");
        let label = row.find("session.create").expect("label");
        let detail = row.find("Start a session").expect("detail");
        assert!(
            detail - label < 40,
            "the detail is {} columns away: {row:?}",
            detail - label
        );
    }
}
