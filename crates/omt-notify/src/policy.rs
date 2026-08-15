//! When a change is worth interrupting somebody for.
//!
//! The hard part of notifying is not sending — it is not sending. A tool that
//! buzzes for everything is one people turn off, and then it does not buzz for
//! the thing that mattered. So this is a separate module with its own tests:
//! the decision is the feature, and the escape sequence is plumbing.

use omt_types::AgentState;

/// What the user asked to hear about.
///
/// The names are the configuration's (`notifications.when`), so the setting
/// and the code cannot drift into meaning different things.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum When {
    /// Only a card waiting on a human.
    #[default]
    NeedsYou,
    /// Anything that changes — finishing included.
    AnyChange,
    /// Nothing.
    Off,
}

impl When {
    /// Parse the configured value.
    ///
    /// An unknown value reads as the default rather than as `Off`: a typo that
    /// silences notifications is a typo nobody discovers, because its symptom
    /// is the absence of something.
    #[must_use]
    pub fn parse(value: &str) -> Self {
        match value.trim().trim_matches('"') {
            "any-change" => Self::AnyChange,
            "off" => Self::Off,
            _ => Self::NeedsYou,
        }
    }
}

/// A moment worth telling somebody about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Moment {
    /// An agent is waiting for a person.
    NeedsYou,
    /// An agent finished, and nobody was watching it.
    Finished,
}

/// Whether this transition is worth saying out loud.
///
/// Rules, in the order they are decided:
///
/// 1. **No change, nothing to say.** The state machine re-reports constantly;
///    every source that reads a screen re-answers on a timer. Notifying on a
///    report rather than a change means notifying every few hundred
///    milliseconds.
/// 2. **Being waited for always counts**, even while somebody is looking
///    straight at the pane. It is a decision to make, not news; a question you
///    can see is still a question that stops the work until you answer it.
/// 3. **Finishing counts only when nobody was watching.** "It finished while
///    you were elsewhere" is the notification; "it finished while you watched
///    it finish" is noise, and it is the noise that gets a tool muted.
#[must_use]
pub fn worth_saying(
    when: When,
    previous: Option<&AgentState>,
    now: &AgentState,
    watching: bool,
) -> Option<Moment> {
    if when == When::Off {
        return None;
    }
    // Compared by shape, not by value: `Working { .. }` carries a description
    // that changes as an agent works, and comparing those would make every
    // step of one long task a fresh event.
    if previous.is_some_and(|before| same_shape(before, now)) {
        return None;
    }
    match now {
        AgentState::Blocked { .. } => Some(Moment::NeedsYou),
        AgentState::Idle
            if when == When::AnyChange
                && !watching
                // Only from *work*. An agent that was idle before this pane
                // was even looked at has not finished anything, and the first
                // reading of a session would otherwise announce itself.
                && previous.is_some_and(|before| {
                    matches!(before, AgentState::Working { .. } | AgentState::Blocked { .. })
                }) =>
        {
            Some(Moment::Finished)
        }
        _ => None,
    }
}

/// Whether two states are the same kind of thing.
fn same_shape(a: &AgentState, b: &AgentState) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b)
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use omt_types::BlockReason;

    use super::*;

    fn blocked() -> AgentState {
        AgentState::Blocked {
            reason: BlockReason::Question,
            interaction: None,
        }
    }

    fn working(what: &str) -> AgentState {
        AgentState::Working {
            detail: Some(what.to_owned()),
        }
    }

    #[test]
    fn a_question_interrupts_you_even_when_you_are_looking_at_it() {
        // It is a decision to make, not news. A question you can see is still
        // a question that has stopped the work until you answer it.
        assert_eq!(
            worth_saying(When::NeedsYou, Some(&working("x")), &blocked(), true),
            Some(Moment::NeedsYou)
        );
    }

    #[test]
    fn the_same_state_reported_again_says_nothing() {
        // Every source that reads a screen re-answers on a timer. Notifying on
        // a report rather than a change is a buzz every few hundred
        // milliseconds, which is how a tool gets muted in its first hour.
        assert_eq!(
            worth_saying(When::NeedsYou, Some(&blocked()), &blocked(), false),
            None
        );
    }

    #[test]
    fn a_long_task_changing_what_it_says_is_not_a_new_event() {
        // `Working` carries a description that moves with the work. Compared
        // by value, every step of one task becomes an event of its own.
        assert_eq!(
            worth_saying(
                When::AnyChange,
                Some(&working("reading files")),
                &working("running tests"),
                false
            ),
            None
        );
    }

    #[test]
    fn finishing_is_worth_saying_only_when_nobody_was_watching() {
        // "It finished while you were elsewhere" is the notification. "It
        // finished while you watched it finish" is the noise.
        let done = AgentState::Idle;
        assert_eq!(
            worth_saying(When::AnyChange, Some(&working("x")), &done, false),
            Some(Moment::Finished)
        );
        assert_eq!(
            worth_saying(When::AnyChange, Some(&working("x")), &done, true),
            None
        );
    }

    #[test]
    fn the_default_setting_does_not_announce_finishing() {
        // Being waited for stops the work; finishing does not. The default is
        // the one worth a buzz, and the other is opt-in.
        let done = AgentState::Idle;
        assert_eq!(
            worth_saying(When::NeedsYou, Some(&working("x")), &done, false),
            None
        );
    }

    #[test]
    fn an_agent_that_was_already_idle_has_not_finished_anything() {
        // Otherwise the first reading of every session announces itself, which
        // is a burst of notifications at the moment somebody starts omt.
        let done = AgentState::Idle;
        assert_eq!(worth_saying(When::AnyChange, None, &done, false), None);
    }

    #[test]
    fn off_means_off() {
        assert_eq!(
            worth_saying(When::Off, Some(&working("x")), &blocked(), false),
            None
        );
    }

    #[test]
    fn a_mistyped_setting_still_notifies() {
        // A typo that silences notifications is a typo nobody discovers: its
        // only symptom is the absence of something.
        assert_eq!(When::parse("needs-yuo"), When::NeedsYou);
        assert_eq!(When::parse("\"any-change\""), When::AnyChange);
        assert_eq!(When::parse("off"), When::Off);
    }
}
