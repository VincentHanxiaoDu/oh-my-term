//! Everything omt writes down, in one list.
//!
//! `docs/architecture/21-data-lifecycle.md` §1.
//!
//! The list exists because the alternative is finding out what omt persists by
//! reading twelve crates, and nobody does that — least of all the person
//! deciding whether to run it on a work machine. An inventory somebody can read
//! in a minute is the difference between "it stores things" and a decision they
//! can actually make.
//!
//! Every entry carries whether it holds sensitive data, and that field is the
//! reason the table is worth keeping in code rather than only in prose. A new
//! artefact added without answering the question fails a test here, and the
//! question is the one that is easiest to skip and most expensive to skip.

use serde::Serialize;

/// One thing omt writes down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Artefact {
    /// What it is.
    pub what: &'static str,
    /// Where it lives, relative to the state directory.
    pub where_: &'static str,
    /// The permission bits it must have.
    pub mode: u32,
    /// Whether it can contain something somebody would not want copied.
    pub sensitive: bool,
    /// Which retention ladder governs it.
    pub retention: crate::retention::Kind,
}

/// Everything omt persists.
pub const INVENTORY: &[Artefact] = &[
    Artefact {
        what: "session tree, layouts and focus",
        where_: "tree/",
        mode: 0o600,
        // The session argv and injected env are in here, which is where a
        // token passed on a command line ends up.
        sensitive: true,
        retention: crate::retention::Kind::BlockMetadata,
    },
    Artefact {
        what: "scrollback chunks",
        where_: "sessions/<id>/scrollback/",
        mode: 0o600,
        // Raw PTY output: the worst offender, and the largest.
        sensitive: true,
        retention: crate::retention::Kind::Output,
    },
    Artefact {
        what: "command block output",
        where_: "sessions/<id>/blocks/",
        mode: 0o600,
        sensitive: true,
        retention: crate::retention::Kind::Output,
    },
    Artefact {
        what: "command blocks",
        where_: "store.db",
        mode: 0o600,
        // The command line itself. `export FOO_TOKEN=…` is a command.
        sensitive: true,
        retention: crate::retention::Kind::BlockMetadata,
    },
    Artefact {
        what: "command history",
        where_: "store.db",
        mode: 0o600,
        sensitive: true,
        retention: crate::retention::Kind::History,
    },
    Artefact {
        what: "agent event log",
        where_: "sessions/<id>/agent.jsonl.zst",
        mode: 0o600,
        sensitive: true,
        retention: crate::retention::Kind::AgentEvents,
    },
    Artefact {
        what: "interaction ledger",
        where_: "interactions.log",
        mode: 0o600,
        // The full tool input, and the user's own free-text answers.
        sensitive: true,
        retention: crate::retention::Kind::Interactions,
    },
    Artefact {
        what: "native session transcript",
        where_: "sessions/<id>/transcript/",
        mode: 0o600,
        sensitive: true,
        retention: crate::retention::Kind::Transcript,
    },
    Artefact {
        what: "search index",
        where_: "store.db",
        mode: 0o600,
        // Sensitive only if something unredacted entered it, which §2 forbids
        // — recorded as sensitive anyway, because "safe as long as another
        // rule holds" is not a thing to write `false` next to.
        sensitive: true,
        retention: crate::retention::Kind::BlockMetadata,
    },
    Artefact {
        what: "usage and cost ledger",
        where_: "store.db",
        mode: 0o600,
        // Counts and totals. The one honest `false` in the table.
        sensitive: false,
        retention: crate::retention::Kind::BlockMetadata,
    },
];

/// What to tell somebody deciding whether to run this on a work machine.
#[must_use]
pub fn summary() -> String {
    let sensitive = INVENTORY.iter().filter(|a| a.sensitive).count();
    format!(
        "omt writes {} kinds of thing to disk, {sensitive} of which can contain something you would not want copied. All are 0600, and everything is deleted on a schedule.",
        INVENTORY.len()
    )
}

/// What omt does and does not encrypt, and why it says so out loud.
///
/// `docs/architecture/21-data-lifecycle.md` §5. omt does not encrypt session
/// content at rest, and this is the decision reviewers push on, so it is worth
/// stating where somebody will find it.
///
/// The key would have to live next to the data. omt starts at login, restores
/// sessions without a prompt, and answers a phone at three in the morning —
/// any key it can use unattended is a key on the same disk, readable by the
/// same uid. That is obfuscation, and shipping it would let people believe
/// something false, which is worse than an honest `0600`.
///
/// The honesty is the feature here. A tool that says "encrypted at rest" and
/// means "we XOR'd it with a key sitting beside it" has taken a decision away
/// from somebody who might otherwise have turned on full-disk encryption.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct AtRest {
    /// Whether session content is encrypted by omt.
    pub content_encrypted: bool,
    /// Whether credentials go to an OS keychain where one exists.
    pub credentials_in_keychain: bool,
    /// What omt relies on instead.
    pub relies_on: &'static str,
}

/// What omt actually does.
#[must_use]
pub const fn at_rest() -> AtRest {
    AtRest {
        content_encrypted: false,
        credentials_in_keychain: true,
        relies_on: "full-disk encryption and 0600 modes",
    }
}

/// What to tell somebody who asks whether omt encrypts their sessions.
///
/// Says no first. An answer that opens with what omt *does* do reads as a yes
/// to somebody skimming, and this is a question people skim.
#[must_use]
pub fn at_rest_summary() -> String {
    "omt does not encrypt session content at rest. It relies on your disk encryption and 0600 file modes, and stores credentials in the OS keychain where there is one. A key omt could use unattended would have to sit on the same disk as the data it protects."
        .to_owned()
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
    fn the_answer_about_encryption_is_no_and_says_so_first() {
        // An answer that opens with what omt does do reads as a yes to
        // somebody skimming, and this is a question people skim.
        let said = at_rest_summary();
        assert!(said.starts_with("omt does not encrypt"), "{said}");
    }

    #[test]
    fn the_summary_says_what_is_relied_on_instead() {
        // "No" on its own is not an answer somebody can act on; "no, rely on
        // FileVault" is.
        let said = at_rest_summary();
        assert!(said.contains("disk encryption"), "{said}");
        assert!(said.contains("0600"), "{said}");
    }

    #[test]
    fn the_reason_is_given_rather_than_the_decision_alone() {
        // This is the decision reviewers push on, and a decision with no
        // reasoning attached gets re-litigated every time somebody new reads
        // it.
        let said = at_rest_summary();
        assert!(said.contains("same disk"), "{said}");
    }

    #[test]
    fn credentials_are_treated_differently_from_content() {
        // A credential is small and can go somewhere a user unlocks; a
        // session's scrollback is neither.
        let state = at_rest();
        assert!(!state.content_encrypted);
        assert!(state.credentials_in_keychain);
    }

    #[test]
    fn nothing_claims_encryption_omt_does_not_do() {
        // A tool that says "encrypted at rest" and means "XOR'd with a key
        // beside it" has taken a decision away from somebody who might have
        // turned on full-disk encryption instead.
        let json = serde_json::to_string(&at_rest()).expect("json");
        assert!(json.contains("\"content_encrypted\":false"), "{json}");
    }


    #[test]
    fn nothing_omt_writes_is_readable_by_anybody_else() {
        for artefact in INVENTORY {
            assert_eq!(artefact.mode & 0o077, 0, "{}", artefact.what);
        }
    }

    #[test]
    fn everything_is_on_a_retention_ladder() {
        // An artefact with no ladder grows until the disk is full, and the
        // disk is somebody's laptop.
        for artefact in INVENTORY {
            let ladder = crate::retention::ladder(artefact.retention);
            assert!(ladder.delete_after_days > 0, "{}", artefact.what);
        }
    }

    #[test]
    fn the_thing_holding_raw_output_is_on_the_shortest_ladder() {
        // It is both the largest and the most sensitive, so it should not be
        // the one that lives longest.
        let scrollback = INVENTORY
            .iter()
            .find(|a| a.what.contains("scrollback"))
            .expect("scrollback");
        let history = INVENTORY
            .iter()
            .find(|a| a.what.contains("history"))
            .expect("history");
        assert!(
            crate::retention::ladder(scrollback.retention).delete_after_days
                < crate::retention::ladder(history.retention).delete_after_days
        );
    }

    #[test]
    fn every_entry_says_where_it_lives() {
        // An inventory that says what without where cannot be checked against
        // a disk, which is the one use somebody has for it when they are
        // worried.
        for artefact in INVENTORY {
            assert!(!artefact.where_.is_empty(), "{}", artefact.what);
            assert!(!artefact.what.is_empty());
        }
    }

    #[test]
    fn the_index_is_marked_sensitive_despite_the_rule_that_protects_it() {
        // "Safe as long as another rule holds" is not a thing to write `false`
        // next to. If redaction ever fails, this is where the failure lands.
        let index = INVENTORY
            .iter()
            .find(|a| a.what.contains("search index"))
            .expect("index");
        assert!(index.sensitive);
    }

    #[test]
    fn something_in_the_table_is_not_sensitive() {
        // Otherwise the field carries no information and nobody reads it — a
        // column that always says the same thing is a column people skip.
        assert!(INVENTORY.iter().any(|a| !a.sensitive));
    }

    #[test]
    fn the_summary_is_something_a_person_can_act_on() {
        let said = summary();
        assert!(said.contains("0600"), "{said}");
        assert!(said.contains("deleted on a schedule"), "{said}");
        assert!(said.contains(&INVENTORY.len().to_string()), "{said}");
    }

    #[test]
    fn the_inventory_is_not_a_stub() {
        // A list of three things would pass every test above and tell nobody
        // anything.
        assert!(INVENTORY.len() >= 10, "{}", INVENTORY.len());
    }
}
