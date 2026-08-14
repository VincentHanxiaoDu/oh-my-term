//! Refusing to open a store this build does not understand.
//!
//! `docs/architecture/21-data-lifecycle.md` §7.1.
//!
//! > Every persisted artefact carries a format version in its first bytes. omt
//! > refuses to open a version it does not understand, and migrates forward
//! > explicitly. There is no best-effort partial parse, ever.
//!
//! The "ever" is the load-bearing word. A best-effort parse of a newer format
//! reads the fields it recognises and silently ignores the rest — and the rest
//! is, by definition, what the newer version added. The store then gets written
//! back without them. Somebody who opened their sessions with an older binary
//! once, by accident, has lost whatever the newer one was recording, and
//! nothing anywhere said so.
//!
//! Refusing is unfriendly for exactly one afternoon. Partial parsing is
//! unfriendly forever, quietly.

use serde::Serialize;

/// The store version this build writes and understands.
pub const CURRENT: u32 = 1;

/// Why a store could not be opened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum Refusal {
    /// Written by a newer omt.
    FromTheFuture {
        /// What the store says.
        store: u32,
        /// What this binary understands.
        binary: u32,
        /// What to do.
        remedy: String,
    },
    /// So old this build no longer carries the migration.
    ///
    /// Distinct from the future case, because the remedy is the opposite: one
    /// is "get a newer omt", the other is "get an older one, or start fresh",
    /// and telling somebody the wrong one costs them the afternoon.
    TooOld {
        /// What the store says.
        store: u32,
        /// The oldest this build can migrate from.
        oldest: u32,
        /// What to do.
        remedy: String,
    },
    /// The version marker is not a version.
    Unreadable {
        /// What was there.
        found: String,
    },
}

/// The oldest version this build knows how to migrate forward from.
pub const OLDEST_MIGRATABLE: u32 = 1;

/// What to do with a store claiming this version.
///
/// `Ok(steps)` is the migrations to run, in order, and an empty list means the
/// store is already current.
///
/// # Errors
/// Refuses anything this build cannot understand, in either direction.
pub fn plan(marker: &str) -> Result<Vec<u32>, Refusal> {
    let trimmed = marker.trim();
    let Ok(version) = trimmed.parse::<u32>() else {
        return Err(Refusal::Unreadable {
            found: trimmed.chars().take(40).collect(),
        });
    };

    if version > CURRENT {
        return Err(Refusal::FromTheFuture {
            store: version,
            binary: CURRENT,
            // Names the action rather than the situation. "Version mismatch"
            // is a statement; "upgrade omt, or point it at a different state
            // directory" is something somebody can do in the next minute.
            remedy: "Upgrade omt, or point this binary at a different state directory with OMT_STATE_DIR.".to_owned(),
        });
    }
    if version < OLDEST_MIGRATABLE {
        return Err(Refusal::TooOld {
            store: version,
            oldest: OLDEST_MIGRATABLE,
            remedy: format!(
                "This build migrates from version {OLDEST_MIGRATABLE} onward. Open it once with an omt that still carries the older migration, or start a fresh state directory."
            ),
        });
    }
    Ok((version + 1..=CURRENT).collect())
}

/// Whether omt will ever read a store it does not fully understand.
///
/// Never. A function rather than a comment, so the rule is something a test can
/// hold rather than something a future reader has to notice.
#[must_use]
pub const fn best_effort_parse_allowed() -> bool {
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

    #[test]
    fn a_current_store_needs_no_migration() {
        assert_eq!(plan(&CURRENT.to_string()), Ok(Vec::new()));
    }

    #[test]
    fn a_store_from_the_future_is_refused_rather_than_partly_read() {
        // A partial parse reads the fields it recognises and ignores the rest
        // — and the rest is, by definition, what the newer version added.
        let refusal = plan("7").expect_err("refused");
        let Refusal::FromTheFuture {
            store,
            binary,
            remedy,
        } = refusal
        else {
            panic!("wrong refusal");
        };
        assert_eq!(store, 7);
        assert_eq!(binary, CURRENT);
        assert!(remedy.contains("Upgrade omt"), "{remedy}");
    }

    #[test]
    fn the_two_directions_are_different_refusals_with_opposite_remedies() {
        // One is "get a newer omt", the other is "get an older one, or start
        // fresh". Telling somebody the wrong one costs them the afternoon.
        let future = plan("999").expect_err("future");
        let past = plan("0").expect_err("past");
        assert!(matches!(future, Refusal::FromTheFuture { .. }));
        assert!(matches!(past, Refusal::TooOld { .. }));
    }

    #[test]
    fn a_marker_that_is_not_a_number_says_what_it_found() {
        // A corrupted or hand-edited marker is a real case, and "invalid
        // version" without the contents leaves somebody guessing whether the
        // file is empty, truncated, or somebody else's.
        let Refusal::Unreadable { found } = plan("not a version").expect_err("refused") else {
            panic!("wrong refusal");
        };
        assert_eq!(found, "not a version");
    }

    #[test]
    fn an_empty_marker_is_unreadable_rather_than_zero() {
        // Treating it as zero would send it down the "too old" path and
        // suggest finding an ancient binary for a file that is simply empty.
        assert!(matches!(plan(""), Err(Refusal::Unreadable { .. })));
        assert!(matches!(plan("   "), Err(Refusal::Unreadable { .. })));
    }

    #[test]
    fn a_long_marker_is_quoted_back_in_a_readable_amount() {
        // The file may be a whole session log if something wrote to the wrong
        // path, and an error message is not a place to print one.
        let Refusal::Unreadable { found } = plan(&"x".repeat(10_000)).expect_err("refused") else {
            panic!("wrong refusal");
        };
        assert!(found.len() <= 40);
    }

    #[test]
    fn surrounding_whitespace_does_not_make_a_version_unreadable() {
        // Every editor that opens the file adds a trailing newline.
        assert_eq!(
            plan(
                " 1
"
            ),
            Ok(Vec::new())
        );
    }

    #[test]
    fn there_is_no_best_effort_parse() {
        // Refusing is unfriendly for exactly one afternoon. Partial parsing is
        // unfriendly forever, quietly.
        assert!(!best_effort_parse_allowed());
    }

    #[test]
    fn every_refusal_names_something_to_do() {
        for marker in ["999", "0"] {
            let refusal = plan(marker).expect_err("refused");
            let json = serde_json::to_value(&refusal).expect("json");
            let remedy = json.get("remedy").and_then(|r| r.as_str()).unwrap_or("");
            assert!(!remedy.is_empty(), "{marker}");
        }
    }

    #[test]
    fn the_refusals_are_distinguishable_on_the_wire() {
        let mut codes = std::collections::BTreeSet::new();
        for marker in ["999", "0", "nonsense"] {
            let refusal = plan(marker).expect_err("refused");
            let json = serde_json::to_value(&refusal).expect("json");
            codes.insert(json["code"].as_str().expect("a code").to_owned());
        }
        assert_eq!(codes.len(), 3);
    }
}
