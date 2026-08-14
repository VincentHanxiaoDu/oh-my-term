//! Redaction, and the ordering that makes it mean anything.
//!
//! `docs/architecture/20-recall-and-usage.md` §5.
//!
//! > **D-R2 — Redaction happens before persistence, and the index is built from
//! > the persisted (redacted) copy. There is no path from raw bytes to `doc`.**
//!
//! The rule is about *ordering*, not about the detector. A perfect detector
//! applied after indexing protects nothing: the secret is already in the index,
//! already in the search results, already in whatever a client cached. A crude
//! detector applied before persistence protects almost everything.
//!
//! So the ordering is enforced by the type rather than by remembering. Raw
//! bytes are a `Raw`, and the only thing that can be indexed is a `Redacted`,
//! and the only way to obtain one is to pass through `redact`. There is no
//! constructor for `Redacted` that takes a string. That is the whole design —
//! a comment saying "call redact first" is a comment somebody will not read.
//!
//! The live screen is never redacted. It shows the truth, because a terminal
//! that hides part of its own output from the person sitting at it is a
//! terminal nobody can debug with. Redaction is about what is *written down*.

use serde::Serialize;

/// Bytes as they arrived. Cannot be indexed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Raw(String);

impl Raw {
    /// Take some output as it was produced.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    /// What the screen shows.
    ///
    /// Named for what it is. The live view is not redacted and must not be —
    /// a terminal that hides part of its own output from the person at the
    /// keyboard is one nobody can debug with.
    #[must_use]
    pub fn for_the_screen(&self) -> &str {
        &self.0
    }
}

/// Text that has been through the detector. The only thing that can be indexed.
///
/// No constructor takes a string. `redact` is the only way in, which is what
/// makes D-R2 a property of the code rather than of somebody's memory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Redacted {
    text: String,
    /// How many spans were replaced.
    pub removed: usize,
}

impl Redacted {
    /// The text, safe to persist and index.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }
}

/// What the detector looks for.
///
/// Prefix-anchored patterns rather than entropy heuristics. An entropy check
/// flags every base64 blob and every hash, which trains people to ignore the
/// feature; a prefix check finds the things that are actually credentials and
/// says nothing about the rest. It misses secrets with no recognisable shape,
/// and that is an honest limitation rather than a hidden one.
const PREFIXES: &[(&str, &str)] = &[
    ("sk-", "an API key"),
    ("ghp_", "a GitHub token"),
    ("github_pat_", "a GitHub token"),
    ("gho_", "a GitHub token"),
    ("xoxb-", "a Slack token"),
    ("xoxp-", "a Slack token"),
    ("AKIA", "an AWS access key"),
    ("AIza", "a Google API key"),
    ("omt_c_", "an omt credential"),
    ("omt_i_", "an omt invite"),
    ("-----BEGIN", "a private key"),
];

/// Environment-assignment shapes, where the name says what the value is.
const SECRET_NAMES: &[&str] = &[
    "PASSWORD",
    "SECRET",
    "TOKEN",
    "APIKEY",
    "API_KEY",
    "PRIVATE_KEY",
    "CREDENTIAL",
];

/// What replaces a redacted span.
///
/// Says what was there rather than blanking it. Somebody reading their own
/// history needs to know a token was on that line — a blank looks like output
/// that never existed, and they go looking for why the command printed nothing.
fn placeholder(what: &str) -> String {
    format!("[redacted: {what}]")
}

/// Run the detector. The only way to obtain a `Redacted`.
#[must_use]
pub fn redact(raw: &Raw) -> Redacted {
    let mut removed = 0usize;
    let mut out = String::with_capacity(raw.0.len());

    for line in raw.0.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        let ending = &line[trimmed.len()..];

        // An assignment whose *name* says it is a secret: the value goes
        // whatever it looks like. This is the case a shape-based detector
        // always misses, and it is the most common one in a shell.
        if let Some((name, _)) = trimmed.split_once('=')
            && let bare = name
                .rsplit([' ', '\t'])
                .next()
                .unwrap_or(name)
                .to_uppercase()
            && !bare.is_empty()
            && SECRET_NAMES.iter().any(|s| bare.contains(s))
        {
            let keep = &trimmed[..=trimmed.find('=').unwrap_or(trimmed.len().saturating_sub(1))];
            out.push_str(keep);
            out.push_str(&placeholder("a secret"));
            out.push_str(ending);
            removed += 1;
            continue;
        }

        let mut rest = trimmed;
        let mut rebuilt = String::new();
        'scan: loop {
            let mut earliest: Option<(usize, &str, &str)> = None;
            for (prefix, what) in PREFIXES {
                if let Some(at) = rest.find(prefix)
                    && earliest.is_none_or(|(best, _, _)| at < best)
                {
                    earliest = Some((at, prefix, what));
                }
            }
            match earliest {
                None => {
                    rebuilt.push_str(rest);
                    break 'scan;
                }
                Some((at, prefix, what)) => {
                    rebuilt.push_str(&rest[..at]);
                    rebuilt.push_str(&placeholder(what));
                    removed += 1;
                    let after = &rest[at + prefix.len()..];
                    // Consume to the end of the token, so the tail of a key is
                    // not left behind — half a credential is still a leak of
                    // where to look.
                    let end = after
                        .find(|c: char| c.is_whitespace() || c == '"' || c == '\'')
                        .unwrap_or(after.len());
                    rest = &after[end..];
                }
            }
        }
        out.push_str(&rebuilt);
        out.push_str(ending);
    }

    Redacted { text: out, removed }
}

/// A document in the index. Can only be built from redacted text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Doc {
    /// Its id.
    pub id: String,
    /// The searchable text, which has been through the detector by
    /// construction.
    pub body: Redacted,
}

impl Doc {
    /// Build a document. Takes `Redacted`, which is the point.
    #[must_use]
    pub fn new(id: impl Into<String>, body: Redacted) -> Self {
        Self {
            id: id.into(),
            body,
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

    #[test]
    fn a_token_does_not_survive_into_a_document() {
        let raw = Raw::new("export KEY=sk-abcdef1234567890 && run");
        let doc = Doc::new("d1", redact(&raw));
        assert!(
            !doc.body.text().contains("sk-abcdef"),
            "{}",
            doc.body.text()
        );
    }

    #[test]
    fn the_screen_still_shows_the_truth() {
        // A terminal that hides part of its own output from the person at the
        // keyboard is one nobody can debug with.
        let raw = Raw::new("token: ghp_realvalue");
        assert!(raw.for_the_screen().contains("ghp_realvalue"));
    }

    #[test]
    fn what_was_there_is_named_rather_than_blanked() {
        // A blank looks like output that never existed, and somebody goes
        // hunting for why the command printed nothing.
        let out = redact(&Raw::new("key: ghp_abc123"));
        assert!(
            out.text().contains("[redacted: a GitHub token]"),
            "{}",
            out.text()
        );
    }

    #[test]
    fn an_assignment_named_as_a_secret_loses_its_value_whatever_it_looks_like() {
        // The case a shape-based detector always misses, and the most common
        // one in a shell.
        for line in [
            "PASSWORD=hunter2",
            "export DB_PASSWORD=correct-horse",
            "MY_API_KEY=12345",
            "some_secret=plainwords",
        ] {
            let out = redact(&Raw::new(line));
            assert_eq!(out.removed, 1, "{line}");
            assert!(out.text().contains("[redacted"), "{line} -> {}", out.text());
        }
    }

    #[test]
    fn the_variable_name_survives_so_the_line_still_reads() {
        let out = redact(&Raw::new("export DB_PASSWORD=hunter2"));
        assert!(out.text().contains("DB_PASSWORD="), "{}", out.text());
        assert!(!out.text().contains("hunter2"));
    }

    #[test]
    fn no_tail_of_a_credential_is_left_behind() {
        // Half a key is still a leak of where to look.
        let out = redact(&Raw::new("AKIAIOSFODNN7EXAMPLE and more"));
        assert!(!out.text().contains("IOSFODNN7EXAMPLE"), "{}", out.text());
        assert!(out.text().contains("and more"), "the rest survives");
    }

    #[test]
    fn several_secrets_on_one_line_all_go() {
        let out = redact(&Raw::new("a sk-one b ghp_two c"));
        assert_eq!(out.removed, 2);
        assert!(!out.text().contains("sk-one"));
        assert!(!out.text().contains("ghp_two"));
    }

    #[test]
    fn ordinary_output_is_left_completely_alone() {
        // A detector that fires on ordinary text trains people to ignore it,
        // and then it protects nothing.
        for line in [
            "Compiling omt v0.1.0",
            "the sky is blue",
            "commit 3f9a1c02deadbeef",
            "GET /api/v1/things 200",
        ] {
            let out = redact(&Raw::new(line));
            assert_eq!(out.removed, 0, "{line} -> {}", out.text());
            assert_eq!(out.text(), line);
        }
    }

    #[test]
    fn line_structure_survives_redaction() {
        // Otherwise search results come back as one run-on line and nobody can
        // tell what came from where.
        let out = redact(&Raw::new("one\nPASSWORD=x\nthree\n"));
        assert_eq!(out.text().lines().count(), 3);
        assert!(out.text().ends_with('\n'));
    }

    #[test]
    fn redacting_twice_changes_nothing_further() {
        let once = redact(&Raw::new("key sk-abc123 end"));
        let twice = redact(&Raw::new(once.text()));
        assert_eq!(twice.text(), once.text());
        assert_eq!(twice.removed, 0);
    }

    #[test]
    fn an_omt_credential_is_treated_as_a_secret_like_anybody_elses() {
        // Nothing is more embarrassing than a tool that redacts other people's
        // tokens and prints its own.
        let out = redact(&Raw::new("joined with omt_c_abcdef"));
        assert!(!out.text().contains("omt_c_abcdef"), "{}", out.text());
    }

    #[test]
    fn a_private_key_header_is_caught() {
        let out = redact(&Raw::new("-----BEGIN RSA PRIVATE KEY-----"));
        assert_eq!(out.removed, 1);
    }
}
