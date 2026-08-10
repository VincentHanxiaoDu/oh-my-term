//! What a plugin is doing, and what makes it stop.
//!
//! `docs/architecture/11-plugins.md` §5. Three running states — healthy,
//! degraded, quarantined — and the transitions between them are the part worth
//! getting right, because each one is a decision about whether to keep trusting
//! code that is misbehaving.
//!
//! **A degraded plugin keeps running.** Timeouts and dropped events are a
//! quality-of-service problem, not a correctness one, and killing a
//! notification sink because it was slow twice is worse than the slowness. What
//! degradation buys is a flag on its contributions in the UI, so a user seeing
//! stale data from a plugin knows why.
//!
//! **A crashing plugin is quarantined, not restarted forever.** The restart
//! budget exists because a plugin that crashes on every event would otherwise
//! be restarted on every event. Quarantine disables it with a notification
//! carrying its last stderr, because a plugin that silently stopped working is
//! the thing users report as "omt broke".
//!
//! **A plugin process exit is never fatal to omt.** Pending requests fail with
//! `plugin_unavailable` — a real answer rather than a hang, which is what a
//! request waiting on a process that will never reply would otherwise be.

use std::collections::VecDeque;

/// Where a plugin is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Found, manifest not yet checked.
    Discovered,
    /// Installed and switched off.
    Disabled,
    /// Running and answering.
    Healthy,
    /// Running, but missing health checks or dropping events.
    ///
    /// Still running: its contributions are flagged, not withdrawn.
    Degraded,
    /// Out of restart budget, switched off, and the user is told.
    Quarantined,
}

/// What the host does with a plugin's exit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Restart {
    /// Leave it stopped.
    Never,
    /// Restart if it exited unexpectedly.
    OnCrash,
    /// Restart whenever it stops.
    Always,
}

/// How many consecutive failed health checks make a plugin degraded.
pub const MISSED_HEALTH_CHECKS: u32 = 3;

/// How long the host waits between a `shutdown` request and `SIGTERM`.
pub const TERM_AFTER_MS: u64 = 5_000;

/// And between `SIGTERM` and `SIGKILL`.
pub const KILL_AFTER_MS: u64 = 10_000;

/// One plugin's running state.
#[derive(Debug, Clone)]
pub struct Health {
    stage: Stage,
    consecutive_failures: u32,
    restart: Restart,
    /// Restart timestamps within the last hour, in milliseconds.
    restarts: VecDeque<u64>,
    max_restarts_per_hour: u32,
}

/// What the host should do after a plugin exits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfterExit {
    /// Start it again.
    Restart,
    /// Leave it stopped, as configured.
    Stay,
    /// Out of budget: disable it and tell the user.
    Quarantine,
}

impl Health {
    /// A plugin that has just started.
    #[must_use]
    pub fn started(restart: Restart, max_restarts_per_hour: u32) -> Self {
        Self {
            stage: Stage::Healthy,
            consecutive_failures: 0,
            restart,
            restarts: VecDeque::new(),
            max_restarts_per_hour,
        }
    }

    /// Where it is.
    #[must_use]
    pub const fn stage(&self) -> Stage {
        self.stage
    }

    /// A health check answered.
    pub fn answered(&mut self) {
        self.consecutive_failures = 0;
        if self.stage == Stage::Degraded {
            // Recovery is automatic. A plugin that has to be restarted by hand
            // after one slow minute is a plugin people switch off.
            self.stage = Stage::Healthy;
        }
    }

    /// A health check was missed or failed.
    pub fn missed(&mut self) {
        self.consecutive_failures += 1;
        if self.consecutive_failures >= MISSED_HEALTH_CHECKS && self.stage == Stage::Healthy {
            self.stage = Stage::Degraded;
        }
    }

    /// A request timed out, or events were dropped.
    ///
    /// Degrades without stopping: this is quality of service, not correctness.
    pub fn degrade(&mut self) {
        if self.stage == Stage::Healthy {
            self.stage = Stage::Degraded;
        }
    }

    /// The plugin's process exited.
    ///
    /// `crashed` distinguishes an unexpected exit from an orderly one, which is
    /// the whole content of `restart = "on-crash"`.
    pub fn exited(&mut self, crashed: bool, now_ms: u64) -> AfterExit {
        let wanted = match self.restart {
            Restart::Never => false,
            Restart::OnCrash => crashed,
            Restart::Always => true,
        };
        if !wanted {
            self.stage = Stage::Disabled;
            return AfterExit::Stay;
        }
        // An hour's window, so a plugin that crashes twice a day is never
        // quarantined and one that crashes in a loop is quarantined in
        // seconds.
        while self
            .restarts
            .front()
            .is_some_and(|at| now_ms.saturating_sub(*at) > 3_600_000)
        {
            self.restarts.pop_front();
        }
        if self.restarts.len() >= self.max_restarts_per_hour as usize {
            self.stage = Stage::Quarantined;
            return AfterExit::Quarantine;
        }
        self.restarts.push_back(now_ms);
        self.stage = Stage::Healthy;
        AfterExit::Restart
    }

    /// How many restarts remain in this hour.
    #[must_use]
    pub fn restarts_left(&self) -> u32 {
        self.max_restarts_per_hour
            .saturating_sub(u32::try_from(self.restarts.len()).unwrap_or(u32::MAX))
    }
}

/// A step in stopping a plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// Ask it to stop.
    Request,
    /// Signal it.
    Term,
    /// End it.
    Kill,
}

/// What to do next when stopping a plugin, given how long it has had.
///
/// An escalation rather than a wait: a plugin that ignores `shutdown` must not
/// be able to hold omt's exit open.
#[must_use]
pub const fn stop_step(elapsed_ms: u64) -> Stop {
    if elapsed_ms < TERM_AFTER_MS {
        Stop::Request
    } else if elapsed_ms < KILL_AFTER_MS {
        Stop::Term
    } else {
        Stop::Kill
    }
}

/// What a pending request gets when the plugin behind it is gone.
///
/// An answer rather than a hang: a request waiting on a process that will never
/// reply is indistinguishable, from the caller's side, from one that is slow.
pub const UNAVAILABLE: &str = "plugin_unavailable";

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn plugin() -> Health {
        Health::started(Restart::OnCrash, 5)
    }

    #[test]
    fn three_missed_health_checks_degrade_it() {
        let mut h = plugin();
        h.missed();
        h.missed();
        assert_eq!(h.stage(), Stage::Healthy, "two is not enough");
        h.missed();
        assert_eq!(h.stage(), Stage::Degraded);
    }

    #[test]
    fn a_degraded_plugin_keeps_running() {
        // Timeouts and drops are quality of service, not correctness. Killing a
        // notification sink because it was slow twice is worse than the
        // slowness.
        let mut h = plugin();
        h.degrade();
        assert_eq!(h.stage(), Stage::Degraded);
        assert_ne!(h.stage(), Stage::Quarantined);
    }

    #[test]
    fn answering_again_recovers_it_without_a_restart() {
        // A plugin that has to be restarted by hand after one slow minute is a
        // plugin people switch off.
        let mut h = plugin();
        for _ in 0..MISSED_HEALTH_CHECKS {
            h.missed();
        }
        h.answered();
        assert_eq!(h.stage(), Stage::Healthy);
    }

    #[test]
    fn a_run_of_misses_must_be_consecutive() {
        // One missed check an hour is a different plugin from three in a row.
        let mut h = plugin();
        h.missed();
        h.missed();
        h.answered();
        h.missed();
        h.missed();
        assert_eq!(h.stage(), Stage::Healthy);
    }

    #[test]
    fn a_crash_within_budget_is_restarted() {
        let mut h = plugin();
        assert_eq!(h.exited(true, 1_000), AfterExit::Restart);
        assert_eq!(h.stage(), Stage::Healthy);
    }

    #[test]
    fn an_orderly_exit_is_not_a_crash_and_is_not_restarted() {
        // What `restart = "on-crash"` means, and the difference between a
        // plugin the user stopped and one that fell over.
        let mut h = plugin();
        assert_eq!(h.exited(false, 1_000), AfterExit::Stay);
        assert_eq!(h.stage(), Stage::Disabled);
    }

    #[test]
    fn a_crash_loop_is_quarantined_rather_than_restarted_forever() {
        // A plugin that crashes on every event would otherwise be restarted on
        // every event.
        let mut h = plugin();
        for i in 0..5 {
            assert_eq!(h.exited(true, i * 100), AfterExit::Restart, "restart {i}");
        }
        assert_eq!(h.exited(true, 600), AfterExit::Quarantine);
        assert_eq!(h.stage(), Stage::Quarantined);
    }

    #[test]
    fn the_restart_budget_is_per_hour_not_for_all_time() {
        // A plugin that crashes twice a day is not a crash loop, and treating
        // it as one quarantines something that works.
        let mut h = plugin();
        for i in 0..5 {
            h.exited(true, i * 100);
        }
        let an_hour_later = 3_600_001 + 500;
        assert_eq!(h.exited(true, an_hour_later), AfterExit::Restart);
    }

    #[test]
    fn restart_never_leaves_a_crash_stopped() {
        let mut h = Health::started(Restart::Never, 5);
        assert_eq!(h.exited(true, 0), AfterExit::Stay);
    }

    #[test]
    fn restart_always_restarts_an_orderly_exit_too() {
        let mut h = Health::started(Restart::Always, 5);
        assert_eq!(h.exited(false, 0), AfterExit::Restart);
    }

    #[test]
    fn what_is_left_of_the_budget_is_answerable() {
        // `plugin.health` reports it, so a user can see a plugin approaching
        // quarantine before it gets there.
        let mut h = plugin();
        assert_eq!(h.restarts_left(), 5);
        h.exited(true, 0);
        assert_eq!(h.restarts_left(), 4);
    }

    #[test]
    fn shutdown_escalates_rather_than_waiting_forever() {
        // A plugin that ignores `shutdown` must not be able to hold omt's exit
        // open.
        assert_eq!(stop_step(0), Stop::Request);
        assert_eq!(stop_step(TERM_AFTER_MS), Stop::Term);
        assert_eq!(stop_step(KILL_AFTER_MS), Stop::Kill);
        assert_eq!(
            stop_step(u64::MAX),
            Stop::Kill,
            "there is no elapsed time that goes back to waiting"
        );
    }
}
