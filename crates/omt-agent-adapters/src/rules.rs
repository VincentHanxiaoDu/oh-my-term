//! What a screen says an agent is doing.
//!
//! A rule is a region, a test, a state and a priority. The highest-priority
//! rule that matches wins, and its state is what the screen says.
//!
//! Rules rather than code because an agent's interface changes on its own
//! schedule, and a release of omt is a bad unit of response to somebody else
//! moving a line. Data also makes a wrong rule a thing a person can read,
//! quote and correct without building anything.
//!
//! Everything here is deliberately about *state* and never about content. What
//! an agent said comes from the agent's own stream or not at all: a matcher
//! that could extract text is a matcher somebody will use to put an invented
//! sentence on a phone.

use crate::regions::Region;

/// What a rule concludes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenState {
    /// It is doing something.
    Working,
    /// It is waiting for a person.
    Blocked,
    /// It is waiting for something to do.
    Idle,
    /// This screen says nothing about the state, and the previous one stands.
    ///
    /// For an agent showing its own history: yesterday's question is on the
    /// screen and it is not being asked. Without this, a scrollback viewer
    /// reads as a live prompt and omt reports an agent as blocked forever.
    Hold,
}

/// One test against a region's text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Test {
    /// Every one of these appears somewhere in the region, case-insensitively.
    Contains(Vec<&'static str>),
    /// Some line in the region begins with this, ignoring leading spaces.
    LineStartsWith(&'static str),
    /// At least one of these holds.
    Any(Vec<Test>),
    /// All of these hold.
    All(Vec<Test>),
    /// This does not hold.
    Not(Box<Test>),
}

impl Test {
    /// Whether this holds for a region's text.
    #[must_use]
    pub fn holds(&self, text: &str) -> bool {
        let lowered = text.to_lowercase();
        self.holds_lowered(&lowered)
    }

    fn holds_lowered(&self, lowered: &str) -> bool {
        match self {
            Self::Contains(needles) => needles
                .iter()
                .all(|needle| lowered.contains(&needle.to_lowercase())),
            Self::LineStartsWith(prefix) => {
                let prefix = prefix.to_lowercase();
                lowered
                    .lines()
                    .any(|line| line.trim_start().starts_with(&prefix))
            }
            Self::Any(tests) => tests.iter().any(|t| t.holds_lowered(lowered)),
            Self::All(tests) => tests.iter().all(|t| t.holds_lowered(lowered)),
            Self::Not(test) => !test.holds_lowered(lowered),
        }
    }
}

/// One rule in an agent's set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// What to call it when explaining a verdict.
    pub id: &'static str,
    /// Where it looks.
    pub region: Region,
    /// What it looks for.
    pub test: Test,
    /// What it concludes.
    pub state: ScreenState,
    /// Which rule wins when two match. Higher is stronger.
    ///
    /// Explicit rather than by order in the list, so a rule can be added in
    /// the place it reads best without changing what anything else does.
    pub priority: u16,
}

/// What a rule set made of a screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    /// The state the winning rule concluded.
    pub state: ScreenState,
    /// Which rule won, for explaining it.
    pub rule: &'static str,
}

/// Apply a set to a screen, strongest rule first.
///
/// `None` when nothing matched, which is not the same as `Hold`: nothing
/// matched means this screen is not one the set describes, and the floor
/// applies. `Hold` means a rule matched and said "not from this screen".
#[must_use]
pub fn read(rules: &[Rule], screen: &str) -> Option<Verdict> {
    let mut best: Option<&Rule> = None;
    for rule in rules {
        if best.is_some_and(|b| b.priority >= rule.priority) {
            continue;
        }
        if rule.test.holds(rule.region.of(screen)) {
            best = Some(rule);
        }
    }
    best.map(|rule| Verdict {
        state: rule.state,
        rule: rule.id,
    })
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn rule(
        id: &'static str,
        priority: u16,
        state: ScreenState,
        test: Test,
        region: Region,
    ) -> Rule {
        Rule {
            id,
            region,
            test,
            state,
            priority,
        }
    }

    #[test]
    fn the_strongest_matching_rule_wins_whatever_order_they_are_in() {
        // Priority rather than position, so a rule can be added where it reads
        // best without silently changing what another one does.
        let rules = vec![
            rule(
                "weak",
                10,
                ScreenState::Idle,
                Test::Contains(vec!["x"]),
                Region::Whole,
            ),
            rule(
                "strong",
                900,
                ScreenState::Blocked,
                Test::Contains(vec!["x"]),
                Region::Whole,
            ),
            rule(
                "middle",
                100,
                ScreenState::Working,
                Test::Contains(vec!["x"]),
                Region::Whole,
            ),
        ];
        let read = read(&rules, "x").expect("something matched");
        assert_eq!(read.rule, "strong");
        assert_eq!(read.state, ScreenState::Blocked);
    }

    #[test]
    fn nothing_matching_is_not_the_same_as_a_rule_saying_nothing() {
        // `None` means this screen is not one the set describes and the floor
        // applies. `Hold` means a rule matched and said "not from this
        // screen", which is a decision.
        let rules = vec![rule(
            "viewer",
            500,
            ScreenState::Hold,
            Test::Contains(vec!["showing detailed transcript"]),
            Region::Whole,
        )];
        assert!(read(&rules, "ordinary output").is_none());
        assert_eq!(
            read(&rules, "showing detailed transcript")
                .expect("matched")
                .state,
            ScreenState::Hold
        );
    }

    #[test]
    fn a_region_is_what_keeps_a_rule_off_the_scrollback() {
        // The same words, in the question and in the history above it.
        let rules = vec![rule(
            "asking",
            900,
            ScreenState::Blocked,
            Test::Contains(vec!["esc to cancel"]),
            Region::AfterLastRule,
        )];
        let asking = "output\n────────────\nesc to cancel\n";
        let scrolled = "esc to cancel\n────────────\n❯ \n";
        assert!(read(&rules, asking).is_some());
        assert!(
            read(&rules, scrolled).is_none(),
            "the rule fired on text above the rule"
        );
    }

    #[test]
    fn every_needle_must_appear_not_just_one() {
        let test = Test::Contains(vec!["do you want", "esc to cancel"]);
        assert!(test.holds("do you want to proceed?  esc to cancel"));
        assert!(!test.holds("do you want to proceed?"));
    }

    #[test]
    fn matching_ignores_case_on_both_sides() {
        assert!(Test::Contains(vec!["Esc To Cancel"]).holds("esc to cancel"));
        assert!(Test::LineStartsWith("❯").holds("   ❯ typing here"));
    }

    #[test]
    fn a_negated_test_is_how_a_rule_excludes_the_case_it_would_get_wrong() {
        // The prompt box means idle — unless the box is showing a question.
        let test = Test::All(vec![
            Test::LineStartsWith("❯"),
            Test::Not(Box::new(Test::Contains(vec!["enter to select"]))),
        ]);
        assert!(test.holds("❯ "));
        assert!(!test.holds("❯ pick one\nenter to select"));
    }

    #[test]
    fn any_and_all_nest() {
        let test = Test::All(vec![
            Test::Contains(vec!["proceed"]),
            Test::Any(vec![
                Test::LineStartsWith("1."),
                Test::LineStartsWith("❯ 1."),
            ]),
        ]);
        assert!(test.holds("do you want to proceed?\n 1. yes"));
        assert!(!test.holds("do you want to proceed?\n yes please"));
    }
}
