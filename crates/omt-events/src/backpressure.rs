//! Falling behind, per subscription.
//!
//! `docs/architecture/07-remote-protocol.md` §6. The rule the whole design
//! turns on:
//!
//! > **Event classes are not equal.** Terminal bytes are lossy. Everything else
//! > is not.
//!
//! A phone that cannot keep up with a firehose session must not lose its
//! `interaction` events, because those are the product. Somebody misses the
//! question their agent is blocked on, the agent sits there, and the terminal
//! that was supposed to make that visible is the reason it was not. So there
//! are two queues per subscription and they behave differently.
//!
//! **Backpressure is per subscription, not per connection.** One busy session
//! must not cost a client its events from a quiet one — a client watching a
//! build and an agent at once would otherwise lose the agent.
//!
//! **The fallback under load is not "drop and hope" but "collapse to state".**
//! Terminal output is the one stream that can always be collapsed: a screenful
//! of redraws and the snapshot they add up to are the same thing, and the
//! snapshot is smaller. A phone on a bad link sees fewer intermediate frames
//! and never a corrupted screen.
//!
//! **Every drop is announced.** There is no path where a client misses
//! something and does not know it. A silent drop turns into a bug report about
//! the wrong component every time.

use serde::Serialize;

/// What to do when a subscription cannot keep up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LagPolicy {
    /// Drop terminal frames, coalesce, then resync. The default.
    Resync,
    /// Never drop; push back upstream.
    ///
    /// Only legal on a local transport. A slow *remote* peer applying flow
    /// control would stall the PTY reader, and then a phone on a bad link
    /// freezes the terminal on the laptop — one client degrading the session
    /// for everybody, including the person sitting in front of it.
    Block,
    /// Drop terminal frames; close the connection rather than lose an event.
    Strict,
}

/// How much a subscription may hold before it is behind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    /// What to do when it is.
    pub on_lag: LagPolicy,
    /// Most events buffered.
    pub max_buffered_events: usize,
    /// Most bytes of terminal payload buffered.
    pub max_buffered_bytes: usize,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            on_lag: LagPolicy::Resync,
            max_buffered_events: 2048,
            max_buffered_bytes: 1024 * 1024,
        }
    }
}

/// Whether a `Block` policy may be used on this transport.
///
/// Refused rather than honoured on a remote one: the failure it causes is
/// remote-shaped and would be diagnosed as "omt froze", not as "this phone's
/// link is bad".
#[must_use]
pub const fn block_is_legal(local: bool) -> bool {
    local
}

/// What happened to something offered to a subscription.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "outcome")]
pub enum Delivery {
    /// Queued.
    Queued,
    /// Terminal bytes were dropped in favour of a snapshot.
    ///
    /// Announced, with how much was skipped, so a client can show "catching
    /// up" rather than silently showing less.
    Coalesced {
        /// How many bytes never went over the wire.
        dropped_bytes: u64,
    },
    /// The session's stream was rebuilt from a snapshot.
    Resynced {
        /// How many bytes were skipped in total.
        dropped_bytes: u64,
    },
    /// The connection must be closed: an event could not be delivered.
    Overloaded,
}

/// What a client's terminal view is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewState {
    /// Receiving bytes continuously.
    Live,
    /// Receiving snapshots under load.
    Coalesced,
    /// Rebuilt from a snapshot; scrollback before it must be fetched.
    Resynced,
}

/// One subscription's two queues.
#[derive(Debug, Clone)]
pub struct Subscription {
    policy: Policy,
    /// Terminal payload waiting, in bytes.
    terminal_bytes: usize,
    /// Events waiting that must not be dropped.
    lossless: usize,
    dropped_bytes: u64,
    state: ViewState,
}

impl Subscription {
    /// A subscription under a policy.
    #[must_use]
    pub const fn new(policy: Policy) -> Self {
        Self {
            policy,
            terminal_bytes: 0,
            lossless: 0,
            dropped_bytes: 0,
            state: ViewState::Live,
        }
    }

    /// What its terminal view is doing.
    #[must_use]
    pub const fn state(&self) -> ViewState {
        self.state
    }

    /// How much terminal output it never received.
    #[must_use]
    pub const fn dropped_bytes(&self) -> u64 {
        self.dropped_bytes
    }

    /// How much is waiting to go out.
    #[must_use]
    pub const fn buffered_bytes(&self) -> usize {
        self.terminal_bytes
    }

    /// Offer terminal output.
    ///
    /// Lossy by design. Past the byte cap the accumulated buffer is replaced by
    /// a snapshot rather than trimmed, because a trimmed byte stream is a
    /// corrupted screen and a snapshot is a correct one.
    pub fn offer_terminal(&mut self, bytes: usize) -> Delivery {
        self.terminal_bytes += bytes;
        if self.terminal_bytes <= self.policy.max_buffered_bytes {
            return Delivery::Queued;
        }
        let dropped = self.terminal_bytes as u64;
        // Collapsed to state: the snapshot is what all those bytes add up to,
        // and it is strictly smaller than a screenful of redraws.
        self.terminal_bytes = 0;
        self.dropped_bytes += dropped;
        self.state = ViewState::Coalesced;
        Delivery::Coalesced {
            dropped_bytes: dropped,
        }
    }

    /// Offer an event that must not be lost.
    ///
    /// Never silently dropped. If this queue overflows the session is resynced,
    /// which is itself announced; if that cannot be delivered either, the
    /// connection is closed with `overloaded`. There is no path where a client
    /// misses an interaction and does not know it.
    pub fn offer_event(&mut self) -> Delivery {
        self.lossless += 1;
        if self.lossless <= self.policy.max_buffered_events {
            return Delivery::Queued;
        }
        match self.policy.on_lag {
            LagPolicy::Strict => Delivery::Overloaded,
            LagPolicy::Block | LagPolicy::Resync => {
                // Terminal bytes go first: they are the cheap thing to give up,
                // and giving them up may be enough to catch up.
                let dropped = self.terminal_bytes as u64;
                self.terminal_bytes = 0;
                self.dropped_bytes += dropped;
                if self.lossless > self.policy.max_buffered_events * 2 {
                    // Giving up the lossy queue was not enough.
                    return Delivery::Overloaded;
                }
                self.state = ViewState::Resynced;
                Delivery::Resynced {
                    dropped_bytes: dropped,
                }
            }
        }
    }

    /// The socket drained.
    pub fn flushed(&mut self, events: usize) {
        self.lossless = self.lossless.saturating_sub(events);
        self.terminal_bytes = 0;
        if self.lossless == 0 && self.state == ViewState::Coalesced {
            self.state = ViewState::Live;
        }
    }
}

/// How long output accumulates before it is flushed, in milliseconds.
///
/// One display frame. Below this a client cannot see the difference, and above
/// it typing starts to feel late.
pub const FLUSH_MS: u64 = 16;

/// Flush early past this much, whatever the timer says.
pub const FLUSH_BYTES: usize = 32 * 1024;

/// When output should go out, given what is already waiting.
///
/// 07 §7's first optimization, and by its own account most of the perceived
/// difference: **the flush timer applies to the second byte onward.** Output
/// arriving at an idle subscription goes immediately. Waiting sixteen
/// milliseconds for a companion that is not coming is sixteen milliseconds
/// added to every prompt, every keystroke echo, and every short command's
/// output — the cases a person notices.
#[must_use]
pub const fn flush_after_ms(already_buffered: usize) -> u64 {
    if already_buffered == 0 {
        // Nothing was waiting: this is the first byte after idle.
        0
    } else {
        FLUSH_MS
    }
}

/// Whether a paste should be sent as one frame.
///
/// Yes, always: four thousand keystroke frames for a paste is the one place
/// where coalescing input is right, and it is a different thing from delaying a
/// keystroke — the bytes are already all present, so nothing is being waited
/// for.
#[must_use]
pub const fn coalesce_paste() -> bool {
    true
}

/// Whether an input frame may be held back.
///
/// Never. Latency of the local echo path dominates perceived quality, and
/// sixteen milliseconds of coalescing on a keystroke is sixteen milliseconds
/// somebody feels.
#[must_use]
pub const fn may_coalesce_input() -> bool {
    false
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn subscription() -> Subscription {
        Subscription::new(Policy {
            max_buffered_events: 4,
            max_buffered_bytes: 1000,
            ..Policy::default()
        })
    }

    #[test]
    fn output_within_the_budget_just_goes_out() {
        let mut s = subscription();
        assert_eq!(s.offer_terminal(500), Delivery::Queued);
        assert_eq!(s.state(), ViewState::Live);
    }

    #[test]
    fn output_past_the_budget_collapses_to_a_snapshot() {
        // Not "drop and hope": a trimmed byte stream is a corrupted screen and
        // a snapshot is a correct one.
        let mut s = subscription();
        s.offer_terminal(900);
        let Delivery::Coalesced { dropped_bytes } = s.offer_terminal(500) else {
            panic!("coalesced");
        };
        assert_eq!(dropped_bytes, 1400);
        assert_eq!(s.state(), ViewState::Coalesced);
    }

    #[test]
    fn what_was_skipped_is_counted_so_the_client_can_be_told() {
        // Every drop is announced. A silent one turns into a bug report about
        // the wrong component.
        let mut s = subscription();
        s.offer_terminal(1500);
        assert_eq!(s.dropped_bytes(), 1500);
    }

    #[test]
    fn an_event_is_never_dropped_to_make_room_for_terminal_bytes() {
        // The rule the whole design turns on: somebody missing the question
        // their agent is blocked on is the failure this exists to prevent.
        let mut s = subscription();
        for _ in 0..4 {
            assert_eq!(s.offer_event(), Delivery::Queued);
        }
    }

    #[test]
    fn a_full_event_queue_gives_up_the_terminal_bytes_first() {
        // They are the cheap thing to give up, and giving them up may be
        // enough to catch up.
        let mut s = subscription();
        s.offer_terminal(900);
        for _ in 0..4 {
            s.offer_event();
        }
        let Delivery::Resynced { dropped_bytes } = s.offer_event() else {
            panic!("resynced");
        };
        assert_eq!(dropped_bytes, 900);
        assert_eq!(s.state(), ViewState::Resynced);
    }

    #[test]
    fn a_client_that_cannot_be_caught_up_is_closed_rather_than_quietly_lied_to() {
        // If a resync cannot be delivered either, the connection is closed with
        // `overloaded`. Continuing would mean a client with a view it believes
        // is current.
        let mut s = subscription();
        for _ in 0..20 {
            s.offer_event();
        }
        assert_eq!(s.offer_event(), Delivery::Overloaded);
    }

    #[test]
    fn a_strict_subscription_closes_rather_than_resyncing() {
        let mut s = Subscription::new(Policy {
            on_lag: LagPolicy::Strict,
            max_buffered_events: 2,
            ..Policy::default()
        });
        s.offer_event();
        s.offer_event();
        assert_eq!(s.offer_event(), Delivery::Overloaded);
    }

    #[test]
    fn blocking_is_refused_on_a_remote_transport() {
        // A slow remote peer applying flow control stalls the PTY reader, and
        // then one phone on a bad link freezes the terminal for the person
        // sitting in front of the laptop.
        assert!(block_is_legal(true));
        assert!(!block_is_legal(false));
    }

    #[test]
    fn draining_returns_a_coalesced_view_to_live() {
        // Otherwise a badge that appeared under one burst of load stays up for
        // ever and stops meaning anything.
        let mut s = subscription();
        s.offer_terminal(1500);
        assert_eq!(s.state(), ViewState::Coalesced);
        s.flushed(0);
        assert_eq!(s.state(), ViewState::Live);
    }

    #[test]
    fn a_resynced_view_does_not_quietly_become_live_again() {
        // `resynced` says something about the scrollback the client holds --
        // that what came before the snapshot is not in the stream -- and that
        // stays true after the queue drains.
        let mut s = subscription();
        for _ in 0..5 {
            s.offer_event();
        }
        s.flushed(5);
        assert_eq!(s.state(), ViewState::Resynced);
    }

    #[test]
    fn typing_is_never_held_back() {
        // Sixteen milliseconds of coalescing on a keystroke is sixteen
        // milliseconds somebody feels.
        assert!(!may_coalesce_input());
    }

    #[test]
    fn the_first_byte_after_idle_goes_out_immediately() {
        // 07 §7 calls this most of the perceived difference. Waiting 16ms for a
        // companion byte that is not coming adds it to every prompt and every
        // echo.
        assert_eq!(flush_after_ms(0), 0);
        assert_eq!(flush_after_ms(1), FLUSH_MS);
    }

    #[test]
    fn a_paste_is_one_frame_and_a_keystroke_is_not() {
        // Different things: a paste's bytes are all present already, so nothing
        // is being waited for. A keystroke held for a companion is latency
        // somebody feels.
        assert!(coalesce_paste());
        assert!(!may_coalesce_input());
    }

    #[test]
    fn the_flush_timer_is_one_display_frame() {
        // Below it a client cannot see the difference; above it typing starts
        // to feel late.
        assert_eq!(FLUSH_MS, 16);
        // A burst flushes early rather than waiting out the timer: 32 KiB is
        // already more than a screenful, and holding it back would trade
        // latency for nothing.
        assert_eq!(FLUSH_BYTES, 32 * 1024);
        assert!(
            FLUSH_BYTES < Policy::default().max_buffered_bytes,
            "the early flush happens well before the collapse-to-snapshot cap, \
             or a subscription would resync where a flush would have done"
        );
    }
}
