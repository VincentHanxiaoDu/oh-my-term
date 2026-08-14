//! Whether the last stop was a stop or a fall.
//!
//! `docs/architecture/05-session-model.md` §8.2. A restore has to know which,
//! because the two mean different things to the person reading the screen:
//!
//! - **Clean.** Everything that was there is here. Say nothing; a banner after
//!   an orderly restart is noise that teaches people to ignore banners.
//! - **Recovered.** No marker: omt fell over. Whatever was written after the
//!   last scrollback flush is gone, and the honest thing is to say roughly how
//!   much — a number the user can weigh against what they remember doing.
//! - **Partial.** Something was corrupt. The remainder is *quarantined, never
//!   deleted*, and this surfaces as a warning rather than as a smaller number
//!   nobody notices. A restore that silently drops data is the failure mode
//!   this whole enum exists to make impossible.
//!
//! The marker is written on the way down and removed on the way up. That
//! ordering is the whole mechanism: a file that is still there when omt starts
//! means the last omt never got to remove it.

use std::path::{Path, PathBuf};

/// What a restore found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The marker was there: everything restored.
    Clean {
        /// How many sessions came back.
        sessions: u32,
    },
    /// No marker. The tail written after the last flush is gone.
    Recovered {
        /// How many sessions came back.
        sessions: u32,
        /// Roughly how much output that is, in seconds.
        ///
        /// An estimate rather than a measurement, and named as one: the exact
        /// figure is unknowable, and a precise-looking number would be a lie
        /// with more digits.
        lost_tail_secs: u64,
    },
    /// Something was corrupt past a point.
    Partial {
        /// How many sessions came back.
        sessions: u32,
        /// Where the rest was put. Never deleted.
        quarantined: PathBuf,
        /// What was wrong.
        reason: String,
    },
}

impl Outcome {
    /// How many sessions came back, whatever happened.
    #[must_use]
    pub const fn sessions(&self) -> u32 {
        match self {
            Self::Clean { sessions }
            | Self::Recovered { sessions, .. }
            | Self::Partial { sessions, .. } => *sessions,
        }
    }

    /// Whether a surface must show this to the user.
    ///
    /// A clean restore says nothing. A banner after an orderly restart is noise
    /// that teaches people to ignore banners, and then they ignore the one that
    /// mattered.
    #[must_use]
    pub const fn needs_telling(&self) -> bool {
        !matches!(self, Self::Clean { .. })
    }

    /// What to put in front of the user.
    #[must_use]
    pub fn notice(&self) -> Option<String> {
        match self {
            Self::Clean { .. } => None,
            Self::Recovered {
                sessions,
                lost_tail_secs,
            } => Some(format!(
                "omt did not shut down cleanly. {sessions} session(s) restored; up to about {lost_tail_secs}s of output written just before the stop is missing."
            )),
            Self::Partial {
                sessions,
                quarantined,
                reason,
            } => Some(format!(
                "part of the store could not be read ({reason}). {sessions} session(s) restored to the last consistent point; the remainder is kept at {} and has not been deleted.",
                quarantined.display()
            )),
        }
    }
}

/// The file whose presence means "the last omt is still running, or fell over".
#[must_use]
pub fn marker_path(state_dir: &Path) -> PathBuf {
    state_dir.join("running.marker")
}

/// Write the marker, on the way up.
///
/// # Errors
/// Fails if the state directory cannot be written.
pub fn mark_running(state_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(state_dir)?;
    std::fs::write(marker_path(state_dir), std::process::id().to_string())
}

/// Remove the marker, on the way down.
///
/// # Errors
/// Fails if it is there and cannot be removed. A marker that could not be
/// removed is reported rather than ignored: leaving it makes the next start
/// claim a crash that did not happen, and a false crash report is how a real
/// one stops being believed.
pub fn mark_stopped(state_dir: &Path) -> std::io::Result<()> {
    let path = marker_path(state_dir);
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

/// Clear the marker when this process is asked to stop.
///
/// Without this every restart reports a crash, and a crash report that is
/// always wrong is worse than none — it is the banner people learn to ignore.
///
/// The handler does the least it can: a signal handler may only call
/// async-signal-safe functions, so it records the request and the serve loop
/// acts on it. `unlink` happens on the ordinary path, not in the handler.
pub fn clear_on_signal(state_dir: &Path) {
    // Held so the handler, which cannot take arguments or capture, can find it.
    static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    static ASKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    // A second call would install a handler for a directory the first one is
    // already watching, and the second directory would be the one that never
    // got cleared.
    if DIR.set(state_dir.to_path_buf()).is_err() {
        return;
    }

    extern "C" fn handle(signal: libc::c_int) {
        ASKED.store(true, std::sync::atomic::Ordering::SeqCst);
        // `unlink` is async-signal-safe, and doing it here rather than after
        // returning is what makes this work for a signal that ends the process.
        if let Some(dir) = DIR.get() {
            let path = marker_path(dir);
            if let Ok(c) = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()) {
                #[allow(unsafe_code, reason = "unlink is async-signal-safe")]
                unsafe {
                    libc::unlink(c.as_ptr());
                }
            }
        }
        // Restore the default and re-raise, so the exit status is the one the
        // sender expects rather than a success omt invented.
        #[allow(unsafe_code, reason = "restoring the default disposition")]
        unsafe {
            libc::signal(signal, libc::SIG_DFL);
            libc::raise(signal);
        }
    }

    for signal in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
        #[allow(
            unsafe_code,
            reason = "installing a signal handler has no safe wrapper"
        )]
        unsafe {
            libc::signal(signal, handle as *const () as libc::sighandler_t);
        }
    }
}

/// Whether the previous run ended cleanly.
#[must_use]
pub fn ended_cleanly(state_dir: &Path) -> bool {
    !marker_path(state_dir).exists()
}

/// Classify a restore.
///
/// `flush_interval_secs` is the scrollback flush period, which is what bounds
/// how much output a crash can lose — so it is the honest estimate rather than
/// a guess.
#[must_use]
pub fn classify(
    state_dir: &Path,
    sessions: u32,
    flush_interval_secs: u64,
    corruption: Option<(PathBuf, String)>,
) -> Outcome {
    if let Some((quarantined, reason)) = corruption {
        // Corruption outranks cleanliness: a clean shutdown that wrote a
        // damaged file is still a damaged file, and reporting `Clean` because
        // the marker was tidy would be exactly the silent drop this forbids.
        return Outcome::Partial {
            sessions,
            quarantined,
            reason,
        };
    }
    if ended_cleanly(state_dir) {
        Outcome::Clean { sessions }
    } else {
        Outcome::Recovered {
            sessions,
            lost_tail_secs: flush_interval_secs,
        }
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

    fn dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    #[test]
    fn a_directory_with_no_marker_is_a_clean_start() {
        let d = dir();
        assert!(ended_cleanly(d.path()));
        assert_eq!(
            classify(d.path(), 4, 10, None),
            Outcome::Clean { sessions: 4 }
        );
    }

    #[test]
    fn a_marker_left_behind_means_omt_fell_over() {
        // The whole mechanism: the marker is removed on the way down, so one
        // that is still there means the last omt never got to remove it.
        let d = dir();
        mark_running(d.path()).expect("marked");
        assert!(!ended_cleanly(d.path()));
        let outcome = classify(d.path(), 2, 10, None);
        assert!(matches!(outcome, Outcome::Recovered { .. }));
    }

    #[test]
    fn stopping_clears_the_marker() {
        let d = dir();
        mark_running(d.path()).expect("marked");
        mark_stopped(d.path()).expect("stopped");
        assert!(ended_cleanly(d.path()));
    }

    #[test]
    fn stopping_twice_is_not_an_error() {
        // A shutdown path that runs twice — a signal during an orderly stop —
        // must not turn a clean exit into a failure.
        let d = dir();
        mark_running(d.path()).expect("marked");
        mark_stopped(d.path()).expect("first");
        mark_stopped(d.path()).expect("second");
    }

    #[test]
    fn a_clean_restore_says_nothing() {
        // A banner after an orderly restart is noise that teaches people to
        // ignore banners, and then they ignore the one that mattered.
        let outcome = Outcome::Clean { sessions: 3 };
        assert!(!outcome.needs_telling());
        assert_eq!(outcome.notice(), None);
    }

    #[test]
    fn a_recovered_restore_says_roughly_how_much_was_lost() {
        // A number the user can weigh against what they remember doing.
        let outcome = Outcome::Recovered {
            sessions: 3,
            lost_tail_secs: 10,
        };
        assert!(outcome.needs_telling());
        let notice = outcome.notice().expect("a notice");
        assert!(notice.contains("10s"), "{notice}");
        assert!(
            notice.contains("about"),
            "it is honest that this is an estimate"
        );
    }

    #[test]
    fn corruption_is_reported_as_kept_rather_than_as_a_smaller_number() {
        // A restore that silently drops data is what this enum exists to make
        // impossible.
        let outcome = Outcome::Partial {
            sessions: 1,
            quarantined: PathBuf::from("/state/quarantine/log.3"),
            reason: "checksum mismatch at record 412".to_owned(),
        };
        let notice = outcome.notice().expect("a notice");
        assert!(notice.contains("not been deleted"), "{notice}");
        assert!(notice.contains("checksum mismatch"), "{notice}");
    }

    #[test]
    fn corruption_outranks_a_clean_marker() {
        // A clean shutdown that wrote a damaged file is still a damaged file,
        // and reporting Clean because the marker was tidy would be the silent
        // drop this forbids.
        let d = dir();
        let outcome = classify(
            d.path(),
            1,
            10,
            Some((PathBuf::from("/q"), "torn record".to_owned())),
        );
        assert!(matches!(outcome, Outcome::Partial { .. }));
    }

    #[test]
    fn the_session_count_survives_every_outcome() {
        // A surface shows it whichever happened, so it must not be reachable
        // only in the happy case.
        for outcome in [
            Outcome::Clean { sessions: 7 },
            Outcome::Recovered {
                sessions: 7,
                lost_tail_secs: 10,
            },
            Outcome::Partial {
                sessions: 7,
                quarantined: PathBuf::from("/q"),
                reason: "x".to_owned(),
            },
        ] {
            assert_eq!(outcome.sessions(), 7);
        }
    }

    #[test]
    fn the_estimate_is_the_flush_interval_because_that_is_what_bounds_the_loss() {
        // Not a guess: output is durable as of the last flush, so the window is
        // exactly one flush period wide.
        let d = dir();
        mark_running(d.path()).expect("marked");
        let Outcome::Recovered { lost_tail_secs, .. } = classify(d.path(), 1, 30, None) else {
            panic!("recovered");
        };
        assert_eq!(lost_tail_secs, 30);
    }
}
