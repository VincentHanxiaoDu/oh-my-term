//! The recognition corpus.
//!
//! `docs/architecture/18-semantic-open.md` §11.1, which calls it the
//! centrepiece. One file per producer of real output, and each case says not
//! only what must match but **what must not**.
//!
//! The negative cases carry the weight. A recogniser that matches everything
//! passes every positive test ever written, and the way this feature actually
//! fails in use is by making ordinary log lines tappable until nobody trusts a
//! highlight. So `"the file is at the top"` and `"warning: unused variable"`
//! are in here on purpose, and each is a line an earlier version of the matcher
//! got wrong.
//!
//! The hostile file exists because a recogniser is fed whatever a remote
//! process decides to print. Those cases are constructed rather than captured:
//! a right-to-left override that renders a path as something else, a homograph
//! URL, a token beginning with `-`, and one longer than any terminal line —
//! which is where a quadratic scanner stops being merely slow.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]

use std::path::PathBuf;

#[derive(serde::Deserialize)]
struct Corpus {
    producer: String,
    cases: Vec<Case>,
}

#[derive(serde::Deserialize)]
struct Case {
    line: String,
    /// The rule that must fire, or `null` for a line that must match nothing.
    rule: Option<String>,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    line_number: Option<u32>,
}

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus/open")
}

fn corpora() -> Vec<(String, Corpus)> {
    let dir = corpus_dir();
    let mut out = Vec::new();
    for entry in
        std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()))
    {
        let path = entry.expect("entry").path();
        if path.extension().is_some_and(|e| e == "json") {
            let text = std::fs::read_to_string(&path).expect("read");
            let parsed: Corpus =
                serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            out.push((path.display().to_string(), parsed));
        }
    }
    assert!(!out.is_empty(), "the corpus directory is empty");
    out
}

#[test]
fn every_corpus_case_recognises_what_it_says_it_does() {
    let mut checked = 0usize;
    for (file, corpus) in corpora() {
        for case in &corpus.cases {
            let fired = omt_open::rules::which(&case.line);
            assert_eq!(
                fired.map(str::to_owned),
                case.rule,
                "{} ({}): `{}`",
                file,
                corpus.producer,
                case.line
            );

            if let Some(expected) = &case.target {
                let matched = omt_open::rules::apply(&case.line)
                    .unwrap_or_else(|| panic!("{file}: `{}` matched no rule", case.line));
                match matched.target {
                    omt_open::Target::Path { raw, line, .. } => {
                        assert_eq!(&raw, expected, "{file}: `{}`", case.line);
                        if case.line_number.is_some() {
                            assert_eq!(line, case.line_number, "{file}: `{}`", case.line);
                        }
                    }
                    other => panic!("{file}: expected a path, got {other:?}"),
                }
            }
            checked += 1;
        }
    }
    // A corpus runner that silently found no files would pass. This is the
    // same failure the licensing gate had, and it is worth one assertion.
    assert!(checked >= 20, "only {checked} cases ran");
}

#[test]
fn the_corpus_carries_negative_cases_and_they_outnumber_nothing() {
    // The negatives are the point. A recogniser that matches everything passes
    // every positive test ever written, and the way this fails in use is by
    // making ordinary log lines tappable until nobody trusts a highlight.
    let mut negatives = 0usize;
    let mut positives = 0usize;
    for (_, corpus) in corpora() {
        for case in &corpus.cases {
            if case.rule.is_none() {
                negatives += 1;
            } else {
                positives += 1;
            }
        }
    }
    assert!(positives > 0);
    assert!(
        negatives >= positives,
        "{negatives} negative cases against {positives} positive"
    );
}

#[test]
fn nothing_hostile_becomes_something_tappable() {
    // Constructed rather than captured: a recogniser is fed whatever a remote
    // process decides to print.
    let (_, hostile) = corpora()
        .into_iter()
        .find(|(name, _)| name.ends_with("hostile.json"))
        .expect("a hostile corpus file");
    for case in &hostile.cases {
        assert!(
            case.rule.is_none(),
            "the hostile file must contain only negative cases"
        );
        assert_eq!(
            omt_open::rules::which(&case.line),
            None,
            "hostile line matched: `{}`",
            case.line
        );
    }
}

#[test]
fn a_pathologically_long_line_does_not_take_pathologically_long() {
    // Where a quadratic scanner stops being merely slow. Sixteen thousand
    // characters is beyond any real terminal line and still has to return.
    let line = format!("--> {}", "a/".repeat(8_000));
    let started = std::time::Instant::now();
    let _ = omt_open::recognize(&line);
    let took = started.elapsed();
    assert!(
        took < std::time::Duration::from_millis(50),
        "a 16k line took {took:?}"
    );
}

#[test]
fn recognition_is_stable_across_a_reflow_of_the_same_text() {
    // §11.2. The same logical line matched at two widths must yield the same
    // target. A link that appears and vanishes as a window is resized is worse
    // than no link at all, and the widths a user drags through are not ones
    // anybody tests by hand.
    for (_, corpus) in corpora() {
        for case in corpus.cases.iter().filter(|c| c.rule.is_some()) {
            let once = omt_open::rules::apply(&case.line);
            let padded = format!("{}   ", case.line);
            let again = omt_open::rules::apply(&padded);
            assert_eq!(
                once.map(|m| m.target),
                again.map(|m| m.target),
                "`{}` matched differently with trailing space",
                case.line
            );
        }
    }
}
