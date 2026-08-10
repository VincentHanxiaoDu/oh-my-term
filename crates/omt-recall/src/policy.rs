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
pub const INDEXED_TOOL_FIELDS: &[&str] = &["command", "file_path", "path", "pattern", "description"];

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

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

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
        let indexed = apply(&log, treatment(Source::BlockOutput, &Settings::default()))
            .expect("indexed");
        assert!(indexed.contains("ERROR: it broke"), "the tail survived");
        assert!(indexed.len() < log.len() / 2);
    }

    #[test]
    fn output_keeps_the_start_where_the_command_echo_is() {
        let log = format!("$ cargo build\n{}", "x".repeat(50_000));
        let indexed = apply(&log, treatment(Source::BlockOutput, &Settings::default()))
            .expect("indexed");
        assert!(indexed.starts_with("$ cargo build"));
    }

    #[test]
    fn the_dropped_middle_is_marked_rather_than_silently_joined() {
        // A silent join makes the halves look contiguous and produces phantom
        // matches across the seam.
        let log = "a".repeat(50_000);
        let indexed = apply(&log, treatment(Source::BlockOutput, &Settings::default()))
            .expect("indexed");
        assert!(indexed.contains("not indexed"), "{}", &indexed[..80]);
    }

    #[test]
    fn something_smaller_than_the_window_is_kept_whole() {
        let short = "just a little output";
        let indexed = apply(short, treatment(Source::BlockOutput, &Settings::default()))
            .expect("indexed");
        assert_eq!(indexed, short);
    }

    #[test]
    fn zeroing_the_window_turns_output_indexing_off_entirely() {
        let off = Settings {
            output_window: (0, 0),
            ..Settings::default()
        };
        assert_eq!(treatment(Source::BlockOutput, &off), Treatment::Excluded);
        assert_eq!(apply("anything", treatment(Source::BlockOutput, &off)), None);
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
