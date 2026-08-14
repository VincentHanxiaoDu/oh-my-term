//! Named regions of a pane's screen.
//!
//! State read from a screen has to be read from *somewhere on it*. "Does this
//! screen contain the words enter to select" is true of a diff, of scrollback,
//! of a file the agent is displaying, and of the question the agent asked ten
//! minutes ago and has since answered. "Does the text after the last
//! horizontal rule contain them" is true when an agent is asking now.
//!
//! That difference is the whole engineering. Everything here is a pure
//! function from the screen's text to a slice of it, so a rule can say where
//! it looks and be tested without a terminal.
//!
//! Deliberately not a general query language. Each region is a shape that
//! agent CLIs actually draw — a prompt box, a rule above a question, the last
//! few lines of output — and adding one is a decision about what agents do,
//! not about what a matcher can express.

/// Where on the screen a rule looks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Region {
    /// Everything on the screen.
    ///
    /// The weakest region and the one to reach for last: it is what makes a
    /// rule fire on scrollback.
    Whole,
    /// The last `n` lines that have anything on them.
    ///
    /// Blank lines are skipped rather than counted, because an agent that pads
    /// its output would otherwise push its own status line out of the region.
    BottomNonEmpty(usize),
    /// Everything below the last horizontal rule.
    ///
    /// Agent CLIs draw a rule above the thing they want an answer to, so this
    /// is "the current question" without knowing what the question is.
    AfterLastRule,
    /// Inside the box an agent draws its input line in.
    ///
    /// Bounded by the rule above it and the next rule below, which is how
    /// these boxes are drawn.
    PromptBox,
}

impl Region {
    /// The part of `screen` this region names.
    #[must_use]
    pub fn of<'a>(&self, screen: &'a str) -> &'a str {
        match self {
            Self::Whole => screen,
            Self::BottomNonEmpty(n) => bottom_non_empty(screen, *n),
            Self::AfterLastRule => after_last_rule(screen),
            Self::PromptBox => prompt_box(screen).unwrap_or(""),
        }
    }
}

/// Whether a line is one an agent drew to separate things.
///
/// Box-drawing horizontals, and only those: a line of dashes is something a
/// person types in a commit message, and a line of equals signs is output from
/// half the tools there are. Three or more, so a single character in a table
/// is not a rule.
#[must_use]
pub fn is_rule(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }
    let run = trimmed.chars().take_while(|c| *c == '─').count();
    if run == 0 {
        return false;
    }
    let rest = trimmed.chars().skip(run).collect::<String>();
    // A rule with a label on it is still a rule, provided the run is long
    // enough that it was drawn rather than typed.
    rest.trim().is_empty() || run >= 3
}

fn line_offsets(screen: &str) -> Vec<usize> {
    let mut offsets = vec![0usize];
    for (i, ch) in screen.char_indices() {
        if ch == '\n' {
            offsets.push(i + 1);
        }
    }
    offsets
}

fn bottom_non_empty(screen: &str, n: usize) -> &str {
    if n == 0 {
        return "";
    }
    let lines: Vec<&str> = screen.lines().collect();
    let offsets = line_offsets(screen);
    let Some(start_index) = lines
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, line)| !line.trim().is_empty())
        .take(n)
        .last()
        .map(|(i, _)| i)
    else {
        return "";
    };
    offsets
        .get(start_index)
        .map_or("", |start| &screen[(*start).min(screen.len())..])
}

fn after_last_rule(screen: &str) -> &str {
    let lines: Vec<&str> = screen.lines().collect();
    let offsets = line_offsets(screen);
    let Some(last) = lines.iter().rposition(|line| is_rule(line)) else {
        return screen;
    };
    offsets
        .get(last + 1)
        .map_or("", |start| &screen[(*start).min(screen.len())..])
}

fn prompt_box(screen: &str) -> Option<&str> {
    let lines: Vec<&str> = screen.lines().collect();
    let offsets = line_offsets(screen);
    // The last rule that has another rule below it is the top of the box.
    let bottom = lines.iter().rposition(|line| is_rule(line))?;
    let top = lines[..bottom].iter().rposition(|line| is_rule(line))?;
    let start = *offsets.get(top + 1)?;
    let end = *offsets.get(bottom)?;
    Some(&screen[start.min(screen.len())..end.min(screen.len())])
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    /// A screen shaped like the ones agent CLIs draw.
    const SCREEN: &str = "\
ran the tests
they passed
────────────────────────────
❯ what next
────────────────────────────
  esc to cancel · enter to select
";

    #[test]
    fn the_prompt_box_is_what_is_between_the_two_rules() {
        // Not the question below it and not the output above it.
        let body = Region::PromptBox.of(SCREEN);
        assert!(body.contains("what next"), "{body:?}");
        assert!(!body.contains("esc to cancel"), "{body:?}");
        assert!(!body.contains("they passed"), "{body:?}");
    }

    #[test]
    fn after_the_last_rule_is_the_thing_being_asked_now() {
        let asked = Region::AfterLastRule.of(SCREEN);
        assert!(asked.contains("esc to cancel"), "{asked:?}");
        assert!(!asked.contains("what next"), "{asked:?}");
    }

    #[test]
    fn a_screen_with_no_rules_is_all_of_it() {
        // Falling back to nothing would make every rule silently never fire on
        // an agent that draws no rules.
        let plain = "just some output\nand more\n";
        assert_eq!(Region::AfterLastRule.of(plain), plain);
    }

    #[test]
    fn the_bottom_lines_skip_the_blank_ones() {
        // An agent that pads its output would otherwise push its own status
        // line out of the region.
        let padded = "status line\n\n\n\n\n";
        assert!(Region::BottomNonEmpty(1).of(padded).contains("status line"));
    }

    #[test]
    fn the_bottom_of_nothing_is_nothing() {
        assert_eq!(Region::BottomNonEmpty(3).of("\n\n\n"), "");
        assert_eq!(Region::BottomNonEmpty(0).of(SCREEN), "");
    }

    #[test]
    fn a_rule_is_drawn_and_not_typed() {
        // A line of dashes is something a person types in a commit message and
        // a line of equals signs is output from half the tools there are.
        assert!(is_rule("────────"));
        assert!(is_rule("─── files ───"));
        assert!(!is_rule("--------"));
        assert!(!is_rule("========"));
        assert!(!is_rule(""));
        assert!(!is_rule("   "));
        assert!(!is_rule("a line with ─ in it"));
    }

    #[test]
    fn the_whole_screen_is_the_last_resort() {
        // Named so a rule that uses it is visibly choosing the weakest region,
        // which is the one that fires on scrollback.
        assert_eq!(Region::Whole.of(SCREEN), SCREEN);
    }
}
