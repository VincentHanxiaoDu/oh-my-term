//! What goes into the index, and what deliberately does not.
//!
//! `docs/architecture/20-recall-and-usage.md` §2.
//!
//! The whole table is a value-per-byte argument. Command text is about sixteen
//! kilobytes a day and is the single most useful thing anybody searches for.
//! Block output is hundreds of megabytes a day and is almost all worthless —
//! but not entirely, because the errors are in it. Indexing everything makes
//! search slow and the results useless; indexing nothing makes it a feature
//! that does not work.
//!
//! Two decisions carry the design.
//!
//! **Output is windowed, not truncated.** The first few kilobytes hold the
//! command echo and the first diagnostics; the last few hold the error and the
//! summary. The hundred megabytes in the middle of a build log are worth
//! nothing. Truncating from the front alone would drop exactly the part
//! somebody is searching for, because errors live at the end.
//!
//! **Reasoning is not indexed by default.** It is high volume, low recall
//! value, and the most privacy-sensitive text an agent produces — somebody
//! searching their own history for "password" should not find an agent's
//! musings about one. Opt-in, because for some people it genuinely is the
//! useful part.

use serde::Serialize;

/// What a piece of text came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// The command somebody typed.
    BlockCommand,
    /// What it printed.
    BlockOutput,
    /// Something the user said to an agent.
    UserMessage,
    /// Something the agent said back.
    AssistantText,
    /// The agent's own thinking.
    Reasoning,
    /// The arguments to a tool call.
    ToolCallInput,
    /// What a tool returned.
    ToolResultOutput,
    /// A path that changed.
    FileChanged,
    /// A question and its answer.
    Interaction,
}

/// What omt will do with one source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Treatment {
    /// Indexed in full.
    Full,
    /// Indexed, capped at a byte count.
    Capped {
        /// How many bytes are kept.
        bytes: usize,
    },
    /// The head and the tail are kept and the middle is dropped.
    Windowed {
        /// From the start.
        head: usize,
        /// From the end.
        tail: usize,
    },
    /// Kept as a filter but never as searchable text.
    StructuredOnly,
    /// Not indexed at all.
    Excluded,
}

/// How the index is configured.
#[derive(Debug, Clone, Copy)]
pub struct Settings {
    /// Head and tail kept from block output. Zero disables output indexing.
    pub output_window: (usize, usize),
    /// Whether an agent's reasoning is indexed.
    pub index_reasoning: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            output_window: (4 * 1024, 8 * 1024),
            index_reasoning: false,
        }
    }
}

/// What omt does with this source under these settings.
#[must_use]
pub fn treatment(source: Source, settings: &Settings) -> Treatment {
    match source {
        // Tiny, and the single highest-value field there is.
        Source::BlockCommand | Source::FileChanged => Treatment::Full,
        Source::BlockOutput => {
            let (head, tail) = settings.output_window;
            if head == 0 && tail == 0 {
                Treatment::Excluded
            } else {
                Treatment::Windowed { head, tail }
            }
        }
        Source::UserMessage | Source::AssistantText => Treatment::Capped { bytes: 16 * 1024 },
        Source::Reasoning => {
            if settings.index_reasoning {
                Treatment::Capped { bytes: 16 * 1024 }
            } else {
                Treatment::Excluded
            }
        }
        // Only the fields that are a query. The rest is payload — `Write`
        // carries whole file bodies, and indexing those is indexing the
        // repository through the side door.
        Source::ToolCallInput => Treatment::Capped { bytes: 2 * 1024 },
        // Bulk output by another name.
        Source::ToolResultOutput => Treatment::StructuredOnly,
        Source::Interaction => Treatment::Capped { bytes: 4 * 1024 },
    }
}

/// Which fields of a tool call are worth indexing.
///
/// A list rather than "everything except": a new tool with a new payload field
/// would otherwise be indexed in full the day it appears, and nobody would
/// notice until the index doubled.
pub const INDEXED_TOOL_FIELDS: &[&str] =
    &["command", "file_path", "path", "pattern", "description"];

/// Apply a treatment to some text.
///
/// Returns `None` when the source is not indexed, so a caller cannot
/// accidentally index an excluded source by ignoring an empty string.
#[must_use]
pub fn apply(text: &str, treatment: Treatment) -> Option<String> {
    match treatment {
        Treatment::Excluded | Treatment::StructuredOnly => None,
        Treatment::Full => Some(text.to_owned()),
        Treatment::Capped { bytes } => Some(clip(text, bytes)),
        Treatment::Windowed { head, tail } => {
            if text.len() <= head + tail {
                return Some(text.to_owned());
            }
            let start = clip(text, head);
            let end = clip_end(text, tail);
            // The gap is marked, so nobody searches the middle of a build log
            // and concludes it is not there. A silent join would make the two
            // halves look contiguous and produce phantom matches across the
            // seam.
            Some(format!(
                "{start}\n[… {} bytes not indexed …]\n{end}",
                text.len() - start.len() - end.len()
            ))
        }
    }
}

/// Take at most `bytes` from the front, on a character boundary.
fn clip(text: &str, bytes: usize) -> String {
    if text.len() <= bytes {
        return text.to_owned();
    }
    let mut end = bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

/// Take at most `bytes` from the end, on a character boundary.
fn clip_end(text: &str, bytes: usize) -> String {
    if text.len() <= bytes {
        return text.to_owned();
    }
    let mut start = text.len() - bytes;
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    text[start..].to_owned()
}

/// The questions §1 says people actually ask, and what answering each needs.
///
/// `docs/architecture/20-recall-and-usage.md` §1. Search designs go wrong when
/// they start from "we have a corpus, let us index it" — the corpus here is
/// mostly `cargo build` output, and indexing it well would produce a search
/// that answers nothing anybody asks.
///
/// Kept as data so the indexing policy can be checked against it. A question
/// whose required source is excluded is a question omt cannot answer, and that
/// is worth finding at build time rather than from somebody who searched and
/// got nothing.
pub const QUESTIONS: &[(&str, &[Source])] = &[
    (
        "Which session was I doing the auth refactor in?",
        &[Source::UserMessage, Source::AssistantText],
    ),
    ("What was that ffmpeg command?", &[Source::BlockCommand]),
    (
        "What did the agent change in payments yesterday?",
        &[Source::FileChanged],
    ),
    ("Where did E0499 come from?", &[Source::BlockOutput]),
    (
        "What did it ask me about the migration?",
        &[Source::Interaction],
    ),
];

/// Whether a question can be answered under these settings.
///
/// A question is answerable when *any* of its sources is indexed as text. Not
/// all of them: "which session was the auth refactor in" is answerable from the
/// prompt alone, and requiring every source would report a working feature as
/// broken.
#[must_use]
pub fn answerable(sources: &[Source], settings: &Settings) -> bool {
    sources.iter().any(|source| {
        matches!(
            treatment(*source, settings),
            Treatment::Full | Treatment::Capped { .. } | Treatment::Windowed { .. }
        )
    })
}

/// Expand a path into everything somebody might search for it by.
///
/// `docs/architecture/20-recall-and-usage.md` §3.4. `crates/omt-term/src/grid.rs`
/// is written as the full path plus its suffix chain plus the stem and
/// extension, so `grid.rs`, `omt-term/src` and `grid` all hit.
///
/// Done by writing the text several ways rather than with a custom tokeniser
/// extension. The extension would be a C shim and a portability problem, and
/// the cost here is about three times the path bytes — which against a
/// megabyte of paths a day is nothing.
///
/// The suffix chain is the part that matters. Nobody searches for a full path:
/// they remember the file and maybe the directory above it, and a tokeniser
/// that only stored the whole string would answer "no results" to `grid.rs`.
#[must_use]
pub fn expand_path(path: &str) -> String {
    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() {
        return String::new();
    }
    let parts: Vec<&str> = trimmed.split('/').filter(|p| !p.is_empty()).collect();
    let mut out: Vec<String> = Vec::new();

    // Every suffix, longest first, so a search for the full path scores against
    // the first token rather than the last.
    for start in 0..parts.len() {
        out.push(parts[start..].join("/"));
    }

    // The stem and extension separately, because `grid` and `rs` are both
    // things people type and neither is a token of any suffix above.
    if let Some(basename) = parts.last()
        && let Some((stem, extension)) = basename.rsplit_once('.')
    {
        if !stem.is_empty() {
            out.push(stem.to_owned());
        }
        if !extension.is_empty() {
            out.push(extension.to_owned());
        }
    }

    out.dedup();
    out.join(" ")
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
    fn a_path_is_searchable_by_the_bit_anybody_remembers() {
        // Nobody searches for a full path: they remember the file and maybe
        // the directory above it.
        let expanded = expand_path("crates/omt-term/src/grid.rs");
        for fragment in [
            "crates/omt-term/src/grid.rs",
            "omt-term/src/grid.rs",
            "src/grid.rs",
            "grid.rs",
            "grid",
            "rs",
        ] {
            assert!(
                expanded.split(' ').any(|token| token == fragment),
                "`{fragment}` is not in `{expanded}`"
            );
        }
    }

    #[test]
    fn the_full_path_comes_first_so_it_scores_highest() {
        let expanded = expand_path("a/b/c.rs");
        assert!(expanded.starts_with("a/b/c.rs"));
    }

    #[test]
    fn a_bare_filename_expands_to_its_stem_and_extension() {
        let expanded = expand_path("grid.rs");
        let tokens: Vec<&str> = expanded.split(' ').collect();
        assert!(tokens.contains(&"grid.rs"));
        assert!(tokens.contains(&"grid"));
        assert!(tokens.contains(&"rs"));
    }

    #[test]
    fn a_file_with_no_extension_still_expands() {
        let expanded = expand_path("scripts/build");
        let tokens: Vec<&str> = expanded.split(' ').collect();
        assert!(tokens.contains(&"scripts/build"));
        assert!(tokens.contains(&"build"));
    }

    #[test]
    fn a_dotfile_is_not_mistaken_for_an_extension() {
        // `.gitignore` has no stem, and emitting an empty token would put a
        // double space in the column and match the empty query.
        let expanded = expand_path(".gitignore");
        assert!(!expanded.contains("  "), "{expanded}");
        assert!(expanded.split(' ').any(|t| t == "gitignore"));
    }

    #[test]
    fn leading_and_repeated_slashes_do_not_produce_empty_tokens() {
        for path in ["/a/b.rs", "a//b.rs", "/a/b.rs/"] {
            let expanded = expand_path(path);
            assert!(!expanded.contains("  "), "{path} -> `{expanded}`");
            assert!(!expanded.starts_with(' '), "{path}");
        }
    }

    #[test]
    fn an_empty_path_expands_to_nothing() {
        assert_eq!(expand_path(""), "");
        assert_eq!(expand_path("/"), "");
    }

    #[test]
    fn the_cost_is_a_small_multiple_of_the_path() {
        // Three times the path bytes against a megabyte of paths a day is
        // nothing; thirty times would not be.
        let path = "a/b/c/d/e/f/g/h.rs";
        let expanded = expand_path(path);
        assert!(
            expanded.len() < path.len() * 6,
            "{} vs {}",
            expanded.len(),
            path.len()
        );
    }

    #[test]
    fn every_question_people_actually_ask_is_answerable_by_default() {
        // The check that matters: a question whose sources are all excluded is
        // one omt cannot answer, and finding that out from somebody who
        // searched and got nothing is the expensive way.
        for (question, sources) in QUESTIONS {
            assert!(
                answerable(sources, &Settings::default()),
                "nothing indexed can answer: {question}"
            );
        }
    }

    #[test]
    fn turning_output_indexing_off_costs_a_specific_question() {
        // And the cost is nameable, which is what makes the setting an
        // informed choice rather than a dial somebody turns and regrets.
        let off = Settings {
            output_window: (0, 0),
            ..Settings::default()
        };
        let lost: Vec<&str> = QUESTIONS
            .iter()
            .filter(|(_, sources)| !answerable(sources, &off))
            .map(|(question, _)| *question)
            .collect();
        assert_eq!(lost, ["Where did E0499 come from?"]);
    }

    #[test]
    fn the_command_question_needs_nothing_but_the_command() {
        // "What was that ffmpeg command?" is the one query that must work even
        // with everything else turned off, because it is the most asked.
        let minimal = Settings {
            output_window: (0, 0),
            index_reasoning: false,
        };
        let (_, sources) = QUESTIONS
            .iter()
            .find(|(q, _)| q.contains("ffmpeg"))
            .expect("the command question");
        assert!(answerable(sources, &minimal));
    }

    #[test]
    fn a_question_needing_only_one_of_its_sources_is_answerable() {
        // Requiring all of them would report a working feature as broken.
        assert!(answerable(
            &[Source::Reasoning, Source::BlockCommand],
            &Settings::default()
        ));
    }

    #[test]
    fn a_question_with_no_indexed_source_is_not_answerable() {
        // Otherwise the check above passes vacuously.
        assert!(!answerable(
            &[Source::Reasoning, Source::ToolResultOutput],
            &Settings::default()
        ));
    }

    #[test]
    fn the_command_is_indexed_in_full_because_it_is_the_point() {
        // Sixteen kilobytes a day, and the single highest-value field.
        assert_eq!(
            treatment(Source::BlockCommand, &Settings::default()),
            Treatment::Full
        );
    }

    #[test]
    fn reasoning_is_not_indexed_unless_somebody_asks() {
        // High volume, low recall value, and the most privacy-sensitive text
        // an agent produces. Somebody searching their history for "password"
        // should not find an agent's musings about one.
        assert_eq!(
            treatment(Source::Reasoning, &Settings::default()),
            Treatment::Excluded
        );
        let opted_in = Settings {
            index_reasoning: true,
            ..Settings::default()
        };
        assert!(matches!(
            treatment(Source::Reasoning, &opted_in),
            Treatment::Capped { .. }
        ));
    }

    #[test]
    fn tool_results_are_kept_as_filters_and_not_as_text() {
        // Bulk output by another name.
        assert_eq!(
            treatment(Source::ToolResultOutput, &Settings::default()),
            Treatment::StructuredOnly
        );
    }

    #[test]
    fn output_keeps_the_end_where_the_errors_are() {
        // Truncating from the front alone drops exactly the part somebody is
        // searching for.
        let log = format!("{}ERROR: it broke", "x".repeat(50_000));
        let indexed =
            apply(&log, treatment(Source::BlockOutput, &Settings::default())).expect("indexed");
        assert!(indexed.contains("ERROR: it broke"), "the tail survived");
        assert!(indexed.len() < log.len() / 2);
    }

    #[test]
    fn output_keeps_the_start_where_the_command_echo_is() {
        let log = format!("$ cargo build\n{}", "x".repeat(50_000));
        let indexed =
            apply(&log, treatment(Source::BlockOutput, &Settings::default())).expect("indexed");
        assert!(indexed.starts_with("$ cargo build"));
    }

    #[test]
    fn the_dropped_middle_is_marked_rather_than_silently_joined() {
        // A silent join makes the halves look contiguous and produces phantom
        // matches across the seam.
        let log = "a".repeat(50_000);
        let indexed =
            apply(&log, treatment(Source::BlockOutput, &Settings::default())).expect("indexed");
        assert!(indexed.contains("not indexed"), "{}", &indexed[..80]);
    }

    #[test]
    fn something_smaller_than_the_window_is_kept_whole() {
        let short = "just a little output";
        let indexed =
            apply(short, treatment(Source::BlockOutput, &Settings::default())).expect("indexed");
        assert_eq!(indexed, short);
    }

    #[test]
    fn zeroing_the_window_turns_output_indexing_off_entirely() {
        let off = Settings {
            output_window: (0, 0),
            ..Settings::default()
        };
        assert_eq!(treatment(Source::BlockOutput, &off), Treatment::Excluded);
        assert_eq!(
            apply("anything", treatment(Source::BlockOutput, &off)),
            None
        );
    }

    #[test]
    fn an_excluded_source_returns_nothing_rather_than_an_empty_string() {
        // So a caller cannot index an excluded source by ignoring a blank.
        assert_eq!(apply("secret thoughts", Treatment::Excluded), None);
        assert_eq!(apply("build log", Treatment::StructuredOnly), None);
    }

    #[test]
    fn clipping_never_splits_a_character() {
        // A cap that lands mid-character produces bytes no tokeniser can read,
        // and the failure is a document that silently indexes as nothing.
        let text = "日本語".repeat(1000);
        for bytes in [1, 2, 3, 4, 5, 100, 999] {
            let out = clip(&text, bytes);
            assert!(out.len() <= bytes);
            assert!(std::str::from_utf8(out.as_bytes()).is_ok());
        }
        for bytes in [1, 2, 3, 4, 5, 100, 999] {
            let out = clip_end(&text, bytes);
            assert!(out.len() <= bytes);
            assert!(std::str::from_utf8(out.as_bytes()).is_ok());
        }
    }

    #[test]
    fn the_indexed_tool_fields_are_a_list_rather_than_an_exclusion() {
        // A new tool with a new payload field would otherwise be indexed in
        // full the day it appears, and nobody would notice until the index
        // doubled.
        assert!(INDEXED_TOOL_FIELDS.contains(&"command"));
        assert!(!INDEXED_TOOL_FIELDS.contains(&"content"));
        assert!(!INDEXED_TOOL_FIELDS.contains(&"new_string"));
    }

    #[test]
    fn a_user_message_is_capped_but_generously() {
        // Prompts are how people remember sessions.
        assert_eq!(
            treatment(Source::UserMessage, &Settings::default()),
            Treatment::Capped { bytes: 16 * 1024 }
        );
    }

    #[test]
    fn a_changed_path_is_indexed_whole_because_it_is_a_top_query() {
        // "what touched payments/" is a top-five question.
        assert_eq!(
            treatment(Source::FileChanged, &Settings::default()),
            Treatment::Full
        );
    }
}
