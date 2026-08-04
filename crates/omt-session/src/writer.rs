//! The writer token: at most one actor may type into a session at a time.
//!
//! Silent last-write-wins is not acceptable here. Two people — or a person and
//! an agent — typing into one shell interleave into a corrupted command line,
//! and the failure is invisible until something destructive runs.
//!
//! The load-bearing field is `epoch`. Every write carries the epoch its sender
//! believed it held, and a stale one is rejected. Without it, input already in
//! flight when the token changed hands lands in somebody else's editing
//! session — which is exactly the corruption the token exists to prevent,
//! arriving through the mechanism meant to prevent it.

use omt_types::{Actor, Timestamp};

/// How long a holder may be idle before anyone else may take the token.
pub const IDLE_TIMEOUT_MS: u64 = 90_000;

/// How long the current holder has to object to a takeover.
pub const TAKEOVER_GRACE_MS: u64 = 5_000;

/// A monotonically increasing token generation.
///
/// Never reused, including across a release of the token. A reused epoch would
/// make a write from two holders ago look current.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Epoch(pub u64);

/// Who holds the input channel, and since when.
#[derive(Debug, Clone, PartialEq)]
pub struct WriterToken {
    /// Who holds it.
    pub holder: Actor,
    /// When they got it.
    pub acquired_at: Timestamp,
    /// The last time they actually typed, in monotonic milliseconds.
    pub last_input_ms: u64,
    /// This holder's generation.
    pub epoch: Epoch,
    /// Whether the holder's terminal size pins the session's.
    pub keep_size: bool,
    /// A takeover in progress, if any.
    pub takeover: Option<Takeover>,
}

/// Somebody is asking for the token while another actor holds it.
#[derive(Debug, Clone, PartialEq)]
pub struct Takeover {
    /// Who wants it.
    pub by: Actor,
    /// When the grace period ends.
    pub grace_until_ms: u64,
}

/// How a session decides who may write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriterPolicy {
    /// How long a holder may be idle before the token is takeable.
    pub idle_timeout_ms: u64,
    /// How long a holder has to object.
    pub takeover_grace_ms: u64,
    /// Whether a single attached client takes the token without asking.
    ///
    /// On by default: one person alone at their own terminal should not have to
    /// acquire permission to type into it.
    pub auto_acquire: bool,
}

impl Default for WriterPolicy {
    fn default() -> Self {
        Self {
            idle_timeout_ms: IDLE_TIMEOUT_MS,
            takeover_grace_ms: TAKEOVER_GRACE_MS,
            auto_acquire: true,
        }
    }
}

/// The writer state of one session.
#[derive(Debug, Clone)]
pub struct WriterState {
    /// `None` means free.
    token: Option<WriterToken>,
    /// The policy in force.
    pub policy: WriterPolicy,
    /// The next epoch to hand out.
    next_epoch: u64,
}

/// Why a write or an acquisition was refused.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum WriterError {
    /// Somebody else holds the token.
    #[error("the writer token is held by {holder:?}")]
    Held {
        /// Who has it.
        holder: Box<Actor>,
        /// Whether they have been idle long enough to be displaced.
        takeable: bool,
    },
    /// The write carried an epoch that is no longer current.
    #[error("stale epoch {carried:?}; the token is now at {current:?}")]
    StaleEpoch {
        /// What the writer thought it held.
        carried: Epoch,
        /// What is actually current.
        current: Epoch,
    },
    /// Nobody holds the token, so there is nothing to write with.
    #[error("no writer token is held")]
    NotHeld,
}

impl Default for WriterState {
    fn default() -> Self {
        Self::new(WriterPolicy::default())
    }
}

impl WriterState {
    /// A free session.
    #[must_use]
    pub fn new(policy: WriterPolicy) -> Self {
        Self {
            token: None,
            policy,
            // Epochs start at one so that `Epoch(0)` — which is also
            // `Epoch::default()` — can never be a live generation. A zero that
            // is both "unset" and "the first holder" is a value nobody can
            // check safely.
            next_epoch: 1,
        }
    }

    /// The current token, if any.
    #[must_use]
    pub const fn token(&self) -> Option<&WriterToken> {
        self.token.as_ref()
    }

    /// Whether anyone holds it.
    #[must_use]
    pub const fn is_free(&self) -> bool {
        self.token.is_none()
    }

    /// Take the token.
    ///
    /// # Errors
    /// Fails if somebody else holds it and `force` was not set — reporting who,
    /// and whether they have been idle long enough that forcing is reasonable.
    pub fn acquire(
        &mut self,
        by: Actor,
        now_ms: u64,
        force: bool,
        keep_size: bool,
    ) -> Result<Epoch, WriterError> {
        if let Some(current) = &self.token {
            if current.holder == by {
                // Re-acquiring is not a takeover, and must not burn an epoch:
                // doing so would invalidate the holder's own in-flight writes.
                return Ok(current.epoch);
            }
            let idle_for = now_ms.saturating_sub(current.last_input_ms);
            let takeable = idle_for >= self.policy.idle_timeout_ms;
            if !force && !takeable {
                return Err(WriterError::Held {
                    holder: Box::new(current.holder.clone()),
                    takeable,
                });
            }
        }

        // A fresh epoch on every change of hands. This is what makes writes
        // already in flight from the previous holder detectably stale rather
        // than silently landing in the new holder's line.
        let epoch = Epoch(self.next_epoch);
        self.next_epoch += 1;
        self.token = Some(WriterToken {
            holder: by,
            acquired_at: Timestamp::now(),
            last_input_ms: now_ms,
            epoch,
            keep_size,
            takeover: None,
        });
        Ok(epoch)
    }

    /// Ask for the token, giving the holder a grace period to object.
    ///
    /// # Errors
    /// Fails if the token is free — there is nothing to take.
    pub fn request_takeover(&mut self, by: Actor, now_ms: u64) -> Result<u64, WriterError> {
        let grace = self.policy.takeover_grace_ms;
        let Some(current) = self.token.as_mut() else {
            return Err(WriterError::NotHeld);
        };
        let until = now_ms + grace;
        current.takeover = Some(Takeover {
            by,
            grace_until_ms: until,
        });
        Ok(until)
    }

    /// Complete a takeover whose grace period has elapsed.
    ///
    /// Returns the new epoch, or `None` if the grace period is still running —
    /// a caller polling this cannot accidentally shorten it.
    pub fn complete_takeover(&mut self, now_ms: u64) -> Option<Epoch> {
        let takeover = self.token.as_ref()?.takeover.clone()?;
        if now_ms < takeover.grace_until_ms {
            return None;
        }
        self.acquire(takeover.by, now_ms, true, false).ok()
    }

    /// The holder objects; the takeover is dropped.
    pub fn cancel_takeover(&mut self) {
        if let Some(t) = self.token.as_mut() {
            t.takeover = None;
        }
    }

    /// Give up the token.
    ///
    /// Only the holder may release it, so a stale client cannot free a token
    /// that has since moved on.
    pub fn release(&mut self, by: &Actor) -> bool {
        if self.token.as_ref().is_some_and(|t| &t.holder == by) {
            self.token = None;
            return true;
        }
        false
    }

    /// Check a write and record the activity.
    ///
    /// # Errors
    /// Fails if nobody holds the token, or if the write carries an epoch that
    /// is no longer current.
    pub fn authorize_write(&mut self, epoch: Epoch, now_ms: u64) -> Result<(), WriterError> {
        let Some(current) = self.token.as_mut() else {
            return Err(WriterError::NotHeld);
        };
        if current.epoch != epoch {
            return Err(WriterError::StaleEpoch {
                carried: epoch,
                current: current.epoch,
            });
        }
        current.last_input_ms = now_ms;
        Ok(())
    }

    /// Whether the token can be taken without forcing.
    #[must_use]
    pub fn is_takeable(&self, now_ms: u64) -> bool {
        self.token
            .as_ref()
            .is_none_or(|t| now_ms.saturating_sub(t.last_input_ms) >= self.policy.idle_timeout_ms)
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;
    use omt_types::{DeviceId, IdentityId};

    fn remote() -> Actor {
        Actor::Remote {
            identity: IdentityId::new(),
            device: DeviceId::new(),
        }
    }

    fn state() -> WriterState {
        WriterState::new(WriterPolicy::default())
    }

    #[test]
    fn a_live_epoch_is_never_the_default_value() {
        // Epoch(0) is also Epoch::default(). If it could be handed out, a
        // caller that forgot to set the field would pass the check.
        let mut w = WriterState::default();
        let first = w.acquire(Actor::Local, 0, false, false).expect("claim");
        assert_ne!(first, Epoch::default());
        assert!(w.authorize_write(Epoch::default(), 0).is_err());
    }

    #[test]
    fn a_free_session_can_be_claimed() {
        let mut w = state();
        assert!(w.is_free());
        w.acquire(Actor::Local, 0, false, false).expect("claim");
        assert!(!w.is_free());
    }

    #[test]
    fn a_second_actor_is_refused_and_told_who_holds_it() {
        // So the refused surface can show "your laptop is typing" rather than
        // appearing to be broken.
        let mut w = state();
        w.acquire(Actor::Local, 0, false, false).expect("first");
        let err = w
            .acquire(remote(), 100, false, false)
            .expect_err("must refuse");
        let WriterError::Held { holder, takeable } = err else {
            panic!("expected Held");
        };
        assert_eq!(*holder, Actor::Local);
        assert!(!takeable, "the holder was typing a moment ago");
    }

    #[test]
    fn a_write_with_a_stale_epoch_is_rejected() {
        // The whole reason the epoch exists: input already in flight when the
        // token changed hands must not land in the new holder's command line.
        let mut w = state();
        let first = w.acquire(Actor::Local, 0, false, false).expect("first");
        let second = w
            .acquire(remote(), 1_000, true, false)
            .expect("forced takeover");
        assert_ne!(first, second);

        let err = w.authorize_write(first, 1_100).expect_err("must reject");
        assert!(
            matches!(err, WriterError::StaleEpoch { carried, current } if carried == first && current == second),
            "{err:?}"
        );
        w.authorize_write(second, 1_100)
            .expect("the new holder writes");
    }

    #[test]
    fn an_epoch_is_never_reused() {
        // A reused epoch would make a write from two holders ago look current.
        let mut w = state();
        let a = w.acquire(Actor::Local, 0, false, false).expect("a");
        w.release(&Actor::Local);
        let b = w.acquire(Actor::Local, 1, false, false).expect("b");
        assert_ne!(a, b, "even the same actor re-acquiring gets a new epoch");
        assert!(b > a);
    }

    #[test]
    fn re_acquiring_does_not_burn_an_epoch() {
        // It would invalidate the holder's own in-flight writes for no reason.
        let mut w = state();
        let a = w.acquire(Actor::Local, 0, false, false).expect("a");
        let again = w.acquire(Actor::Local, 10, false, false).expect("again");
        assert_eq!(a, again);
        w.authorize_write(a, 20).expect("still valid");
    }

    #[test]
    fn writing_without_the_token_is_refused() {
        let mut w = state();
        assert!(matches!(
            w.authorize_write(Epoch(1), 0),
            Err(WriterError::NotHeld)
        ));
    }

    #[test]
    fn an_idle_holder_can_be_displaced_without_forcing() {
        // Somebody who walked away must not hold a session hostage.
        let mut w = state();
        w.acquire(Actor::Local, 0, false, false).expect("first");
        let much_later = IDLE_TIMEOUT_MS + 1;
        assert!(w.is_takeable(much_later));
        w.acquire(remote(), much_later, false, false)
            .expect("takeable once idle");
    }

    #[test]
    fn typing_keeps_the_token_alive() {
        // Otherwise a long editing session with pauses would be stolen mid-way.
        let mut w = state();
        let e = w.acquire(Actor::Local, 0, false, false).expect("first");
        w.authorize_write(e, IDLE_TIMEOUT_MS - 1).expect("typed");
        assert!(
            !w.is_takeable(IDLE_TIMEOUT_MS + 1),
            "the idle clock restarted from the last keystroke"
        );
    }

    #[test]
    fn a_takeover_gives_the_holder_a_grace_period() {
        let mut w = state();
        w.acquire(Actor::Local, 0, false, false).expect("first");
        let until = w.request_takeover(remote(), 1_000).expect("request");
        assert_eq!(until, 1_000 + TAKEOVER_GRACE_MS);
        assert!(
            w.complete_takeover(1_100).is_none(),
            "polling must not shorten the grace period"
        );
        assert_eq!(
            w.token().expect("still held").holder,
            Actor::Local,
            "and the holder still has it"
        );
    }

    #[test]
    fn a_takeover_completes_once_the_grace_period_elapses() {
        let mut w = state();
        w.acquire(Actor::Local, 0, false, false).expect("first");
        let taker = remote();
        w.request_takeover(taker.clone(), 1_000).expect("request");
        let epoch = w
            .complete_takeover(1_000 + TAKEOVER_GRACE_MS)
            .expect("completes");
        assert_eq!(w.token().expect("held").holder, taker);
        assert_eq!(w.token().expect("held").epoch, epoch);
    }

    #[test]
    fn the_holder_can_object_and_keep_the_token() {
        let mut w = state();
        w.acquire(Actor::Local, 0, false, false).expect("first");
        w.request_takeover(remote(), 1_000).expect("request");
        w.cancel_takeover();
        assert!(w.complete_takeover(999_999).is_none());
        assert_eq!(w.token().expect("held").holder, Actor::Local);
    }

    #[test]
    fn taking_over_a_free_session_is_not_a_takeover() {
        // There is nobody to give a grace period to.
        let mut w = state();
        assert!(matches!(
            w.request_takeover(Actor::Local, 0),
            Err(WriterError::NotHeld)
        ));
    }

    #[test]
    fn only_the_holder_may_release() {
        // A stale client must not free a token that has since moved on.
        let mut w = state();
        w.acquire(Actor::Local, 0, false, false).expect("first");
        assert!(!w.release(&remote()), "somebody else cannot release it");
        assert!(!w.is_free());
        assert!(w.release(&Actor::Local));
        assert!(w.is_free());
    }

    #[test]
    fn a_free_token_is_takeable() {
        let w = state();
        assert!(w.is_takeable(0));
    }

    #[test]
    fn keep_size_travels_with_the_token() {
        // Which client's dimensions pin the session is a property of who is
        // typing, not a separate setting to keep in sync.
        let mut w = state();
        w.acquire(Actor::Local, 0, false, true).expect("claim");
        assert!(w.token().expect("held").keep_size);
    }
}
