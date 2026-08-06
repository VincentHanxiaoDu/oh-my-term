//! Firing scheduled jobs.
//!
//! The schedule logic lives in `omt-recall` and is tested there — what is here
//! is the loop that consults it and the part that actually starts something.
//! Kept separate deliberately: deciding *whether* a job is due is a pure
//! function over time, and mixing it with process spawning would make it
//! testable only by waiting.

use crate::state::State;

/// How often the scheduler looks for work.
///
/// Once a second. A job's resolution is seconds at best — nothing here fires on
/// a sub-second boundary — and a tighter loop would spend a core discovering
/// that nothing is due.
pub const TICK: std::time::Duration = std::time::Duration::from_secs(1);

/// Jobs that came due on one tick.
///
/// Returned rather than run, so the decision can be tested without spawning
/// anything. A job is marked as started here: a scheduler that decided and then
/// failed to record it would fire the same job again on the next tick, forever.
pub fn due_now(state: &State, now_secs: u64) -> Vec<omt_recall::Job> {
    let Ok(mut jobs) = state.jobs() else {
        return Vec::new();
    };
    let mut due = Vec::new();
    for schedule in jobs.iter_mut() {
        if schedule.should_fire(now_secs).is_ok() {
            schedule.started(now_secs);
            due.push(schedule.job.clone());
        }
    }
    due
}

/// Record how a job ended.
///
/// Separate from starting it because the gap between them is where a job
/// actually runs, and a failure count that was written at start time would
/// count attempts rather than failures.
pub fn finished(state: &State, name: &str, succeeded: bool) {
    let Ok(mut jobs) = state.jobs() else {
        return;
    };
    if let Some(schedule) = jobs.iter_mut().find(|s| s.job.name == name) {
        schedule.finished(succeeded);
    }
}

/// Run the scheduler until the process ends.
///
/// Each due job is started in its own thread. A job that hangs must not stop
/// every other job from firing, which is exactly what running them on the
/// scheduler thread would do.
pub fn spawn(state: State) {
    std::thread::spawn(move || {
        loop {
            let now = seconds_since_epoch();
            for job in due_now(&state, now) {
                let state = state.clone();
                std::thread::spawn(move || {
                    let ok = run_job(&job);
                    finished(&state, &job.name, ok);
                });
            }
            std::thread::sleep(TICK);
        }
    });
}

/// Actually run one job's command.
fn run_job(job: &omt_recall::Job) -> bool {
    // Through a shell, because `run` is a command line the user wrote and they
    // expect pipes and redirection to work — the same bargain every scheduler
    // makes, and the reason a job is a high-consequence thing to create.
    std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(&job.run)
        .current_dir(&job.workspace)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Wall-clock seconds, which is what a daily trigger is expressed in.
fn seconds_since_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn job(name: &str, seconds: u64) -> omt_recall::Job {
        omt_recall::Job {
            name: name.to_owned(),
            workspace: "/tmp".to_owned(),
            run: "true".to_owned(),
            trigger: omt_recall::Trigger::Every { seconds },
            enabled: true,
        }
    }

    fn state_with(jobs: Vec<omt_recall::Job>) -> State {
        let state = State::default();
        {
            let mut held = state.jobs().expect("jobs");
            for j in jobs {
                held.push(omt_recall::Schedule::new(j));
            }
        }
        state
    }

    #[test]
    fn a_new_job_is_due_immediately() {
        // Otherwise enabling a job appears to do nothing for an hour, and
        // people conclude it is broken.
        let state = state_with(vec![job("nightly", 3600)]);
        assert_eq!(due_now(&state, 1_000).len(), 1);
    }

    #[test]
    fn a_job_that_just_fired_is_not_due_again() {
        // The bug this prevents is the worst one a scheduler has: firing the
        // same job every tick because nothing recorded that it started.
        let state = state_with(vec![job("nightly", 3600)]);
        assert_eq!(due_now(&state, 1_000).len(), 1);
        assert_eq!(due_now(&state, 1_001).len(), 0, "it fired twice");
    }

    #[test]
    fn a_job_becomes_due_again_once_its_interval_has_passed() {
        let state = state_with(vec![job("often", 60)]);
        due_now(&state, 1_000);
        finished(&state, "often", true);
        assert_eq!(due_now(&state, 1_070).len(), 1);
    }

    #[test]
    fn a_job_still_running_does_not_start_a_second_copy() {
        // Two copies of a build racing on the same worktree is worse than a
        // skipped run, so a job that has not reported back never re-fires.
        let state = state_with(vec![job("slow", 1)]);
        due_now(&state, 1_000);
        assert!(
            due_now(&state, 2_000).is_empty(),
            "a second copy started while the first was still running"
        );
    }

    #[test]
    fn a_disabled_job_never_fires() {
        let mut j = job("off", 60);
        j.enabled = false;
        let state = state_with(vec![j]);
        assert!(due_now(&state, 1_000).is_empty());
    }

    #[test]
    fn a_manual_job_is_never_due_on_its_own() {
        let mut j = job("manual", 60);
        j.trigger = omt_recall::Trigger::Manual;
        let state = state_with(vec![j]);
        assert!(due_now(&state, 1_000).is_empty());
    }

    #[test]
    fn a_missed_window_is_skipped_rather_than_caught_up() {
        // Firing six hours of hourly runs the moment a laptop opens is how an
        // automation becomes something people turn off entirely.
        let state = state_with(vec![job("hourly", 3600)]);
        due_now(&state, 1_000);
        finished(&state, "hourly", true);
        let after_a_long_sleep = due_now(&state, 1_000 + 6 * 3600);
        assert_eq!(
            after_a_long_sleep.len(),
            1,
            "a missed window was caught up rather than skipped"
        );
    }

    #[test]
    fn a_failure_is_counted_only_once_the_job_has_finished() {
        // Counting at start time would count attempts, not failures.
        let state = state_with(vec![job("flaky", 60)]);
        due_now(&state, 1_000);
        assert_eq!(
            state.jobs().expect("jobs")[0].state.consecutive_failures,
            0
        );
        finished(&state, "flaky", false);
        assert_eq!(
            state.jobs().expect("jobs")[0].state.consecutive_failures,
            1
        );
    }

    #[test]
    fn a_success_clears_the_failure_run() {
        let state = state_with(vec![job("flaky", 60)]);
        finished(&state, "flaky", false);
        finished(&state, "flaky", true);
        assert_eq!(
            state.jobs().expect("jobs")[0].state.consecutive_failures,
            0
        );
    }

    #[test]
    fn finishing_a_job_that_does_not_exist_is_not_a_panic() {
        // A job removed while it was running is an ordinary race, not a bug.
        let state = state_with(Vec::new());
        finished(&state, "gone", true);
    }
}
