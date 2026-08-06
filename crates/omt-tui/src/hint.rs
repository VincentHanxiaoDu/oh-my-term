//! The hint line: one sentence, at the bottom, that tells you what to do next.
//!
//! The problem it solves is the one every multiplexer has: the keys are
//! invisible. tmux is unusable until somebody tells you about `Ctrl-B`, and the
//! manual is not on screen at the moment you need it.
//!
//! Two rules keep it from becoming noise. It says **one** thing, because a row
//! of six hints is a row nobody reads. And it says the thing that is true
//! *now* — a hint that is always there stops being read within a minute, which
//! is why this is computed from state rather than printed once.

/// What the terminal underneath is doing, as far as a hint needs to know.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Situation {
    /// Whether the prefix key has been pressed and is waiting for the next one.
    pub prefix_armed: bool,
    /// How many agent threads are waiting on a human.
    pub blocked: usize,
    /// Whether an agent is running at all.
    pub agent_running: bool,
    /// How long this session has been open, in seconds.
    pub age_secs: u64,
    /// Whether anyone is attached from somewhere else.
    pub remote_clients: usize,
    /// Whether the user has already used the prefix at least once.
    ///
    /// The one piece of history that matters: somebody who has used it knows it
    /// exists, and repeating the introduction to them is the definition of
    /// nagging.
    pub has_used_prefix: bool,
}

/// The hint to show, or nothing.
///
/// `None` is a real answer and the common one. A status line that always has
/// something in it is a status line that is never read.
#[must_use]
pub fn hint_for(s: &Situation) -> Option<&'static str> {
    // Ordered by urgency, and the order is the design: whatever is most likely
    // to be the reason you are looking at the screen wins the single slot.
    if s.prefix_armed {
        return Some("d detach · c new · n next · ? all keys");
    }
    if s.blocked > 1 {
        return Some("agents are waiting on you — ^A a to answer");
    }
    if s.blocked == 1 {
        return Some("an agent is waiting on you — ^A a to answer");
    }
    // Only while it is still plausibly someone's first minute. After that the
    // absence of the hint is itself information: nothing needs you.
    if !s.has_used_prefix && s.age_secs < FIRST_MINUTE_SECS {
        return Some("^A d detaches · ^A ? shows every key");
    }
    if s.remote_clients > 0 && !s.agent_running {
        // Worth saying unprompted: somebody typing on a screen they think is
        // private should be told it is not.
        return Some("someone else is attached to this session");
    }
    None
}

/// How long "just started" lasts.
const FIRST_MINUTE_SECS: u64 = 60;

/// Render the hint into a line of exactly `cols` cells, dimmed.
///
/// Padded to the full width so it overwrites whatever the previous hint left
/// behind — a shorter hint drawn over a longer one leaves the tail of the old
/// one on screen, which reads as garbage rather than as a stale hint.
#[must_use]
pub fn render_hint(hint: Option<&str>, cols: u16) -> String {
    let cols = cols as usize;
    let text = hint.unwrap_or("");
    let mut line = String::from("\x1b[2m");
    let visible: String = text.chars().take(cols).collect();
    line.push_str(&visible);
    for _ in visible.chars().count()..cols {
        line.push(' ');
    }
    line.push_str("\x1b[0m");
    line
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "in a test, expect() is the assertion")]
mod tests {
    use super::*;

    fn situation() -> Situation {
        Situation {
            prefix_armed: false,
            blocked: 0,
            agent_running: false,
            age_secs: 5,
            remote_clients: 0,
            has_used_prefix: false,
        }
    }

    #[test]
    fn a_new_session_is_told_how_to_leave() {
        // The single thing somebody needs and cannot discover: every
        // multiplexer is unusable until you know the prefix.
        let hint = hint_for(&situation()).expect("a hint");
        assert!(hint.contains('d'), "{hint}");
    }

    #[test]
    fn somebody_who_knows_the_prefix_is_not_told_again() {
        // Repeating the introduction to somebody who has used it is nagging,
        // and a hint that is always there stops being read.
        let s = Situation {
            has_used_prefix: true,
            ..situation()
        };
        assert_eq!(hint_for(&s), None);
    }

    #[test]
    fn the_introduction_stops_after_the_first_minute() {
        let s = Situation {
            age_secs: 600,
            ..situation()
        };
        assert_eq!(hint_for(&s), None);
    }

    #[test]
    fn a_waiting_agent_outranks_the_introduction() {
        // It is the reason the person is looking at the screen.
        let s = Situation {
            blocked: 1,
            ..situation()
        };
        assert!(hint_for(&s).expect("a hint").contains("waiting"));
    }

    #[test]
    fn the_wording_matches_how_many_are_waiting() {
        // "an agent is waiting" when three are is a small lie that costs
        // trust in every other thing the status line says.
        let one = Situation {
            blocked: 1,
            ..situation()
        };
        let many = Situation {
            blocked: 3,
            ..situation()
        };
        assert_ne!(hint_for(&one), hint_for(&many));
    }

    #[test]
    fn an_armed_prefix_beats_everything() {
        // The keystroke is half-finished. Anything else on that line is a
        // distraction from the only decision being made right now.
        let s = Situation {
            prefix_armed: true,
            blocked: 5,
            ..situation()
        };
        assert!(hint_for(&s).expect("a hint").contains("detach"));
    }

    #[test]
    fn being_watched_is_said_out_loud() {
        // Somebody typing on a screen they believe is private should be told
        // that it is not.
        let s = Situation {
            has_used_prefix: true,
            age_secs: 600,
            remote_clients: 1,
            ..situation()
        };
        assert!(hint_for(&s).expect("a hint").contains("attached"));
    }

    #[test]
    fn a_quiet_session_says_nothing_at_all() {
        let s = Situation {
            has_used_prefix: true,
            age_secs: 600,
            ..situation()
        };
        assert_eq!(hint_for(&s), None);
    }

    #[test]
    fn the_line_is_padded_to_the_full_width() {
        // A shorter hint drawn over a longer one leaves the tail of the old
        // one on screen, which reads as garbage rather than as a stale hint.
        let line = render_hint(Some("hi"), 20);
        let visible = line.replace("\x1b[2m", "").replace("\x1b[0m", "");
        assert_eq!(visible.chars().count(), 20, "{visible:?}");
    }

    #[test]
    fn a_hint_too_long_for_the_terminal_is_cut_not_wrapped() {
        // Wrapping would scroll the screen by one line and push the terminal's
        // own last row out of view.
        let line = render_hint(Some("a much longer hint than fits"), 10);
        let visible = line.replace("\x1b[2m", "").replace("\x1b[0m", "");
        assert_eq!(visible.chars().count(), 10);
    }

    #[test]
    fn no_hint_still_clears_the_line() {
        let line = render_hint(None, 8);
        let visible = line.replace("\x1b[2m", "").replace("\x1b[0m", "");
        assert_eq!(visible, "        ");
    }
}
