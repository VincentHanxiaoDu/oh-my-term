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
pub fn hint_for(s: &Situation) -> Option<String> {
    hint_for_within(s, u16::MAX)
}

/// The same, kept inside a width.
///
/// The width matters for exactly one hint — the leader strip, which is a menu
/// and loses entries off the end when it does not fit.
#[must_use]
pub fn hint_for_within(s: &Situation, cols: u16) -> Option<String> {
    // The *resolved* leader, never the literal `^A` these strings used to
    // carry. A hint naming a key the user has not got is worse than no hint:
    // they press it, something else happens, and they stop trusting the row.
    let p = leader();
    // Ordered by urgency, and the order is the design: whatever is most likely
    // to be the reason you are looking at the screen wins the single slot.
    if s.prefix_armed {
        return Some(pending_strip_within(cols));
    }
    if s.blocked > 1 {
        return Some(format!("agents are waiting on you — {p} a to answer"));
    }
    if s.blocked == 1 {
        return Some(format!("an agent is waiting on you — {p} a to answer"));
    }
    // Only while it is still plausibly someone's first minute. After that the
    // absence of the hint is itself information: nothing needs you.
    if !s.has_used_prefix && s.age_secs < FIRST_MINUTE_SECS {
        return Some(format!("{p} d detaches · {p} ? shows every key"));
    }
    if s.remote_clients > 0 && !s.agent_running {
        // Worth saying unprompted: somebody typing on a screen they think is
        // private should be told it is not.
        return Some("someone else is attached to this session".to_owned());
    }
    None
}

/// The which-key strip: every leader key, generated from the binding table.
///
/// Generated rather than written out, so a binding that exists and is never
/// advertised is not expressible. It listed four of the nine keys for as long
/// as it was a hand-typed string — `sessions`, `agents`, `close` and the
/// palette were all reachable and none of them were on screen.
///
/// [16 §3.3](../../../docs/architecture/16-input-and-keymap.md) asks for it
/// **after 250 ms, not instantly**: somebody executing `⟨leader⟩ s` from
/// muscle memory never sees a flash, and somebody who forgot gets the menu.
/// The delay is the caller's — this only says what the strip reads.
#[must_use]
pub fn pending_strip() -> String {
    pending_strip_within(u16::MAX)
}

/// The same, kept inside a width.
///
/// Built to fit rather than built and then cut. `render_hint` truncates, so a
/// strip wider than the terminal loses its *last* entries — and the last
/// entries are the ones added most recently, which is how adding four
/// direction keys silently removed `sessions` and `agents` from the only place
/// they are ever advertised.
///
/// Dropping is from the end, so the order of `LEADER_KEYS` is a priority
/// order: what a person reaches for most often is listed first and survives a
/// narrow terminal. When something is dropped the strip says so, because a
/// menu that quietly shows less than it has is worse than one that admits it.
#[must_use]
pub fn pending_strip_within(cols: u16) -> String {
    let head = format!("{} ▸ ", leader());
    let cols = cols as usize;
    let entries: Vec<String> = crate::app::LEADER_KEYS
        .iter()
        .map(|(key, label)| format!("  {key} {label}"))
        .collect();

    let full: String = std::iter::once(head.clone())
        .chain(entries.iter().cloned())
        .collect();
    if full.chars().count() <= cols {
        return full;
    }

    // The overflow marker has to fit too, or dropping an entry to make room
    // for it drops another.
    const MORE: &str = "  … ^B ?";
    let mut out = head;
    for entry in &entries {
        if out.chars().count() + entry.chars().count() + MORE.chars().count() > cols {
            break;
        }
        out.push_str(entry);
    }
    out.push_str(MORE);
    out
}

/// How long the leader is held before the strip appears.
pub const PENDING_STRIP_DELAY_MS: u64 = 250;

/// The prefix as a person reads it, e.g. `^B`.
#[must_use]
pub fn leader() -> String {
    format!("^{}", crate::app::PREFIX_KEY.to_ascii_uppercase())
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
    fn the_strip_advertises_every_key_the_leader_binds() {
        // The bug this exists to prevent: the strip was a hand-typed string
        // listing four of nine bindings, so `sessions`, `agents`, `close` and
        // the palette were reachable and invisible.
        let strip = pending_strip();
        for (key, label) in crate::app::LEADER_KEYS {
            assert!(strip.contains(*label), "`{label}` is missing from {strip}");
            assert!(
                crate::app::leader_action(*key).is_some(),
                "`{key}` is advertised and does nothing"
            );
        }
    }

    #[test]
    fn no_leader_key_works_without_being_advertised() {
        // The other direction, which is the half that catches drift: a binding
        // added to the match and forgotten in the table would otherwise ship
        // as a key nobody is ever told about.
        for byte in b'!'..=b'~' {
            let key = byte as char;
            if crate::app::leader_action(key).is_none() {
                continue;
            }
            assert!(
                crate::app::LEADER_KEYS.iter().any(|(k, _)| *k == key)
                    // The aliases tmux users arrive with. They do the same
                    // thing as a listed key and listing both would double the
                    // strip's width to say one thing twice.
                    || matches!(key, 'n' | 's' | 'c' | 'o' | '?')
                    // The other three directions, advertised as `hjkl focus`
                    // by the entry for `h`. Four separate entries spent the
                    // strip's entire width and pushed `sessions` off the end
                    // of the screen — a key somebody reaches for far more
                    // often than any single arrow.
                    || matches!(key, 'j' | 'k' | 'l'),
                "`{key}` is bound and never advertised"
            );
        }
    }

    #[test]
    fn an_armed_prefix_shows_that_strip_and_not_some_other_text() {
        let s = Situation {
            prefix_armed: true,
            ..situation()
        };
        assert_eq!(hint_for(&s), Some(pending_strip()));
    }

    #[test]
    fn a_hint_names_the_leader_that_is_actually_bound() {
        // It said `^A` while the binding was `^B` for exactly as long as
        // nothing checked. A hint naming a key the user has not got is worse
        // than no hint at all.
        let hint = hint_for(&situation()).expect("a hint");
        assert!(hint.contains(&leader()), "{hint}");
        assert_eq!(
            leader(),
            format!("^{}", crate::app::PREFIX_KEY.to_ascii_uppercase())
        );
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

#[cfg(test)]
#[allow(clippy::expect_used, reason = "in a test, expect() is the assertion")]
mod strip_width_tests {
    use super::*;

    #[test]
    fn a_narrow_strip_keeps_the_first_keys_and_says_there_are_more() {
        // Built to fit rather than built and then cut. `render_hint`
        // truncates, so a strip wider than the terminal loses its *last*
        // entries — which is how adding four direction keys silently removed
        // `sessions` and `agents` from the only place they are advertised.
        let narrow = pending_strip_within(40);
        assert!(narrow.chars().count() <= 40, "{narrow:?}");
        assert!(narrow.contains("detach"), "{narrow:?}");
        assert!(
            narrow.contains('…'),
            "it dropped keys and did not say so: {narrow:?}"
        );
    }

    #[test]
    fn a_wide_strip_says_everything_and_adds_no_marker() {
        let wide = pending_strip_within(u16::MAX);
        for (_, label) in crate::app::LEADER_KEYS {
            assert!(wide.contains(label), "`{label}` missing from {wide:?}");
        }
        assert!(!wide.contains('…'), "{wide:?}");
    }

    #[test]
    fn the_order_of_the_table_is_the_order_things_survive() {
        // Dropping is from the end, so `LEADER_KEYS` is a priority order and
        // what a person reaches for most often is listed first.
        let first = crate::app::LEADER_KEYS.first().expect("a key").1;
        for cols in [30u16, 40, 60, 80] {
            let strip = pending_strip_within(cols);
            assert!(
                strip.contains(first) || strip.chars().count() <= 12,
                "at {cols} columns the first key is gone: {strip:?}"
            );
        }
    }

    #[test]
    fn a_strip_with_no_room_at_all_still_says_where_the_keys_are() {
        // Nothing fits, and the answer is still not a blank row.
        let tiny = pending_strip_within(1);
        assert!(tiny.contains('?') || tiny.contains('▸'), "{tiny:?}");
    }
}
