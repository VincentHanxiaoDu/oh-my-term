//! Runs that start without somebody being there.
//!
//! An automation types into a terminal on the user's behalf while they are
//! asleep, which is a category of thing that has to be conservative about
//! *not* firing. Three rules follow from that, and each one is a test:
//! a missed window is skipped rather than caught up, a run that is still going
//! blocks the next, and a schedule that keeps failing turns itself off.

use serde::{Deserialize, Serialize};

/// When a schedule fires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Trigger {
    /// Every so many seconds.
    Every {
        /// The interval.
        seconds: u64,
    },
    /// At a fixed time each day, in seconds past midnight, local time.
    Daily {
        /// Seconds past midnight.
        at_secs: u32,
    },
    /// Only when something asks.
    Manual,
}

/// What a schedule does when it fires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Job {
    /// Its name, for a log and a listing.
    pub name: String,
    /// The workspace it runs in.
    pub workspace: String,
    /// The prompt or command it runs.
    pub run: String,
    /// When it fires.
    pub trigger: Trigger,
    /// Whether it is on.
    pub enabled: bool,
}

/// How many consecutive failures turn a schedule off.
///
/// A job that fails every time is a job producing noise on a cadence, and a
/// notification the user learns to dismiss is worse than one that stops.
pub const FAILURES_BEFORE_DISABLING: u32 = 3;

/// What happened to a schedule's last runs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobState {
    /// When it last fired, in seconds since the epoch.
    pub last_fired: Option<u64>,
    /// Whether a run is still going.
    pub running: bool,
    /// How many times in a row it has failed.
    pub consecutive_failures: u32,
    /// Why it was turned off, if it was.
    pub disabled_reason: Option<String>,
}

/// Why a run did not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Skipped {
    /// It is not due yet.
    NotDue,
    /// It is switched off.
    Disabled,
    /// The previous run has not finished.
    ///
    /// Not queued. Two runs of the same job in one workspace would fight over
    /// the same files, and a backlog that built up overnight would all fire at
    /// once in the morning.
    StillRunning,
}

/// One schedule and the state of its runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schedule {
    /// What it runs.
    pub job: Job,
    /// How its runs have gone.
    pub state: JobState,
}

impl Schedule {
    /// A schedule that has never run.
    #[must_use]
    pub fn new(job: Job) -> Self {
        Self {
            job,
            state: JobState::default(),
        }
    }

    /// Whether it should start now.
    ///
    /// A window that was missed — the machine was asleep, or omt was not
    /// running — is **skipped, not caught up**. Firing six hours of hourly runs
    /// the moment a laptop opens is how an automation becomes something people
    /// turn off entirely.
    pub fn should_fire(&self, now_secs: u64) -> Result<(), Skipped> {
        if !self.job.enabled {
            return Err(Skipped::Disabled);
        }
        if self.state.running {
            return Err(Skipped::StillRunning);
        }
        match self.job.trigger {
            Trigger::Manual => Err(Skipped::NotDue),
            Trigger::Every { seconds } => {
                let seconds = seconds.max(1);
                match self.state.last_fired {
                    // Never run: due now rather than one interval from now, so
                    // enabling a job does something visible.
                    None => Ok(()),
                    Some(last) if now_secs.saturating_sub(last) >= seconds => Ok(()),
                    Some(_) => Err(Skipped::NotDue),
                }
            }
            Trigger::Daily { at_secs } => {
                let today = now_secs - (now_secs % 86_400);
                let due = today + u64::from(at_secs);
                if now_secs < due {
                    return Err(Skipped::NotDue);
                }
                match self.state.last_fired {
                    Some(last) if last >= due => Err(Skipped::NotDue),
                    _ => Ok(()),
                }
            }
        }
    }

    /// Record that a run started.
    pub fn started(&mut self, now_secs: u64) {
        self.state.last_fired = Some(now_secs);
        self.state.running = true;
    }

    /// Record that a run finished.
    ///
    /// A run of failures turns the schedule off rather than continuing: a
    /// notification the user learns to dismiss is worse than one that stops.
    pub fn finished(&mut self, succeeded: bool) {
        self.state.running = false;
        if succeeded {
            self.state.consecutive_failures = 0;
            return;
        }
        self.state.consecutive_failures += 1;
        if self.state.consecutive_failures >= FAILURES_BEFORE_DISABLING {
            self.job.enabled = false;
            self.state.disabled_reason = Some(format!(
                "turned off after {} consecutive failures",
                self.state.consecutive_failures
            ));
        }
    }

    /// Turn it back on, forgetting the failures that stopped it.
    pub fn reenable(&mut self) {
        self.job.enabled = true;
        self.state.consecutive_failures = 0;
        self.state.disabled_reason = None;
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

    fn every(seconds: u64) -> Schedule {
        Schedule::new(Job {
            name: "nightly".to_owned(),
            workspace: "/w".to_owned(),
            run: "cargo test".to_owned(),
            trigger: Trigger::Every { seconds },
            enabled: true,
        })
    }

    #[test]
    fn a_new_schedule_is_due_immediately() {
        // Enabling a job should do something visible, not nothing for an hour.
        assert_eq!(every(3600).should_fire(1_000), Ok(()));
    }

    #[test]
    fn it_is_not_due_again_until_the_interval_has_passed() {
        let mut s = every(3600);
        s.started(1_000);
        s.finished(true);
        assert_eq!(s.should_fire(2_000), Err(Skipped::NotDue));
        assert_eq!(s.should_fire(1_000 + 3600), Ok(()));
    }

    #[test]
    fn a_missed_window_is_skipped_not_caught_up() {
        // Firing six hours of hourly runs the moment a laptop opens is how an
        // automation becomes something people turn off entirely.
        let mut s = every(3600);
        s.started(0);
        s.finished(true);

        // The machine was asleep for a day.
        assert_eq!(s.should_fire(86_400), Ok(()), "one run is due");
        s.started(86_400);
        s.finished(true);
        assert_eq!(
            s.should_fire(86_400 + 60),
            Err(Skipped::NotDue),
            "and exactly one — the backlog was not queued"
        );
    }

    #[test]
    fn a_run_that_is_still_going_blocks_the_next() {
        // Two runs of one job in one workspace fight over the same files.
        let mut s = every(1);
        s.started(0);
        assert_eq!(s.should_fire(1_000), Err(Skipped::StillRunning));
        s.finished(true);
        assert_eq!(s.should_fire(1_000), Ok(()));
    }

    #[test]
    fn a_schedule_that_keeps_failing_turns_itself_off() {
        // A notification the user learns to dismiss is worse than one that
        // stops.
        let mut s = every(60);
        for _ in 0..FAILURES_BEFORE_DISABLING {
            s.started(0);
            s.finished(false);
        }
        assert!(!s.job.enabled);
        assert_eq!(s.should_fire(999_999), Err(Skipped::Disabled));
        assert!(
            s.state
                .disabled_reason
                .as_deref()
                .is_some_and(|r| r.contains("consecutive")),
            "and it says why: {:?}",
            s.state.disabled_reason
        );
    }

    #[test]
    fn one_success_forgives_the_failures_before_it() {
        // A flaky job that mostly works should not accumulate its way to off.
        let mut s = every(60);
        s.started(0);
        s.finished(false);
        s.started(60);
        s.finished(false);
        s.started(120);
        s.finished(true);
        assert_eq!(s.state.consecutive_failures, 0);

        s.started(180);
        s.finished(false);
        assert!(s.job.enabled, "the count restarted");
    }

    #[test]
    fn a_disabled_schedule_can_be_turned_back_on() {
        let mut s = every(60);
        for _ in 0..FAILURES_BEFORE_DISABLING {
            s.started(0);
            s.finished(false);
        }
        s.reenable();
        assert!(s.job.enabled);
        assert_eq!(s.state.consecutive_failures, 0);
        assert!(s.state.disabled_reason.is_none());
    }

    #[test]
    fn a_manual_job_never_fires_on_its_own() {
        let s = Schedule::new(Job {
            name: "on demand".to_owned(),
            workspace: "/w".to_owned(),
            run: "deploy".to_owned(),
            trigger: Trigger::Manual,
            enabled: true,
        });
        assert_eq!(s.should_fire(999_999), Err(Skipped::NotDue));
    }

    #[test]
    fn a_daily_job_fires_once_a_day() {
        let mut s = Schedule::new(Job {
            name: "morning".to_owned(),
            workspace: "/w".to_owned(),
            run: "git fetch".to_owned(),
            // 09:00.
            trigger: Trigger::Daily { at_secs: 32_400 },
            enabled: true,
        });

        let day = 86_400 * 100;
        assert_eq!(
            s.should_fire(day + 30_000),
            Err(Skipped::NotDue),
            "too early"
        );
        assert_eq!(s.should_fire(day + 32_400), Ok(()));
        s.started(day + 32_400);
        s.finished(true);
        assert_eq!(
            s.should_fire(day + 50_000),
            Err(Skipped::NotDue),
            "already ran today"
        );
        assert_eq!(s.should_fire(day + 86_400 + 32_400), Ok(()), "tomorrow");
    }

    #[test]
    fn an_interval_of_zero_does_not_spin() {
        // A misconfigured job must not become a busy loop.
        let mut s = every(0);
        s.started(100);
        s.finished(true);
        assert_eq!(s.should_fire(100), Err(Skipped::NotDue));
        assert_eq!(s.should_fire(101), Ok(()));
    }
}
