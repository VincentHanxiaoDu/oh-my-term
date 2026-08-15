//! An agent's conversation, as rows in a panel.
//!
//! The terminal grid shows what an agent *painted*, which is a redrawn box
//! that scrolls away. The transcript is what it said and did, kept as records
//! — and until now only the web client could read it, so somebody at a
//! keyboard had the worse view of the two.
//!
//! This turns entries into lines. It knows nothing about where they are drawn
//! and nothing about how they were obtained, so the same rows answer a pane in
//! the terminal and, eventually, anything else that wants them.

/// One entry, in the shape a list needs.
///
/// Deliberately not the wire type: this is about *rendering*, and a renderer
/// that took the wire type would grow a match arm every time the protocol
/// gained a payload. What it needs is who spoke and what they said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Said {
    /// Who or what: `you`, the agent, a tool, a file.
    pub who: Who,
    /// What was said or done.
    pub text: String,
}

/// Who is speaking in a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Who {
    /// A person.
    You,
    /// omt, on somebody's behalf. Never shown as a person.
    Onward,
    /// The agent's own words.
    Agent,
    /// The agent's reasoning, which is a different kind of claim from its
    /// speech and is marked as one.
    Thinking,
    /// A tool it ran.
    Tool,
    /// A file that changed.
    File,
    /// Something omt has no shape for, said as itself rather than dropped.
    Other,
}

impl Who {
    /// The mark this speaker wears.
    ///
    /// Shape rather than colour, and each one distinct at a glance: reading a
    /// conversation is following who is talking, and a column of identical
    /// bullets is a wall of text.
    #[must_use]
    pub const fn mark(self) -> &'static str {
        match self {
            Self::You => "\u{203a}",
            // Marked apart from a person's turn. A synthetic turn shown as a
            // human one is omt putting words in somebody's mouth.
            Self::Onward => "\u{2192}",
            Self::Agent => "\u{25cf}",
            Self::Thinking => "\u{2022}",
            Self::Tool => "\u{2318}",
            Self::File => "\u{270e}",
            Self::Other => "\u{b7}",
        }
    }
}

/// Wrap entries into lines that fit a width.
///
/// Wrapped rather than truncated: a transcript is prose and cutting it at the
/// margin loses the end of every sentence. Wrapped on spaces where there are
/// any, and hard otherwise — a path with no spaces in it still has to fit.
#[must_use]
pub fn lines(said: &[Said], cols: u16) -> Vec<String> {
    let width = usize::from(cols).max(8);
    let mut out = Vec::new();
    for entry in said {
        let mark = entry.who.mark();
        let mut first = true;
        for line in entry.text.lines() {
            for chunk in wrap(line, width.saturating_sub(2)) {
                out.push(if first {
                    first = false;
                    format!("{mark} {chunk}")
                } else {
                    // Continuations are indented under the mark, so a
                    // paragraph reads as one thing rather than as several
                    // unattributed lines.
                    format!("  {chunk}")
                });
            }
        }
        if first {
            // An entry with no text at all still gets a row: a tool call that
            // said nothing happened, and a gap in the list would hide it.
            out.push(format!("{mark} \u{2026}"));
        }
    }
    out
}

/// Break a line into pieces no wider than `width` columns.
fn wrap(line: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }
    let mut out = Vec::new();
    let mut current = String::new();
    let mut used = 0usize;
    for word in line.split_whitespace() {
        let w: usize = word.chars().map(columns).sum();
        if used > 0 && used + 1 + w > width {
            out.push(std::mem::take(&mut current));
            used = 0;
        }
        if w > width {
            // A single word wider than the line — a path, a hash, a URL.
            // Broken hard rather than left to overrun.
            for ch in word.chars() {
                let c = columns(ch);
                if used + c > width {
                    out.push(std::mem::take(&mut current));
                    used = 0;
                }
                current.push(ch);
                used += c;
            }
            continue;
        }
        if used > 0 {
            current.push(' ');
            used += 1;
        }
        current.push_str(word);
        used += w;
    }
    if !current.is_empty() || out.is_empty() {
        out.push(current);
    }
    out
}

/// How many columns a character takes.
const fn columns(ch: char) -> usize {
    let c = ch as u32;
    let wide = (c >= 0x1100 && c <= 0x115F)
        || (c >= 0x2E80 && c <= 0xA4CF)
        || (c >= 0xAC00 && c <= 0xD7A3)
        || (c >= 0xF900 && c <= 0xFAFF)
        || (c >= 0xFE30 && c <= 0xFE6F)
        || (c >= 0xFF00 && c <= 0xFF60)
        || (c >= 0xFFE0 && c <= 0xFFE6)
        || (c >= 0x1F300 && c <= 0x1F64F)
        || (c >= 0x1F900 && c <= 0x1F9FF);
    if wide { 2 } else { 1 }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn said(who: Who, text: &str) -> Said {
        Said {
            who,
            text: text.to_owned(),
        }
    }

    #[test]
    fn each_speaker_is_marked_differently() {
        // Reading a conversation is following who is talking, and a column of
        // identical bullets is a wall of text.
        let marks = [
            Who::You,
            Who::Onward,
            Who::Agent,
            Who::Thinking,
            Who::Tool,
            Who::File,
        ];
        let mut seen: Vec<&str> = marks.iter().map(|w| w.mark()).collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), marks.len(), "two speakers share a mark");
    }

    #[test]
    fn a_synthetic_turn_is_not_shown_as_a_person() {
        // omt sends turns on somebody's behalf. Rendering one as a human turn
        // is omt putting words in their mouth.
        assert_ne!(Who::You.mark(), Who::Onward.mark());
    }

    #[test]
    fn prose_is_wrapped_rather_than_cut_at_the_margin() {
        // A transcript is prose. Truncating loses the end of every sentence,
        // which is where agents put the answer.
        let long = "the quick brown fox jumps over the lazy dog and keeps going well past the edge";
        let out = lines(&[said(Who::Agent, long)], 24);
        assert!(out.len() > 1, "it was not wrapped: {out:?}");
        assert!(
            out.join(" ").contains("past the edge"),
            "the end was lost: {out:?}"
        );
        for line in &out {
            assert!(line.chars().count() <= 24, "{line:?}");
        }
    }

    #[test]
    fn continuations_are_indented_under_the_mark() {
        // A paragraph has to read as one thing rather than as several
        // unattributed lines.
        let out = lines(
            &[said(Who::Agent, "one two three four five six seven eight")],
            16,
        );
        assert!(out[0].starts_with(Who::Agent.mark()));
        assert!(out[1].starts_with("  "), "{:?}", out[1]);
    }

    #[test]
    fn a_word_wider_than_the_line_is_broken_rather_than_left_to_overrun() {
        // A path, a hash, a URL. Overrunning paints into whatever is beside
        // it.
        let path = "/Users/somebody/very/deep/directory/structure/with/a/long/file.rs";
        for line in lines(&[said(Who::File, path)], 20) {
            assert!(line.chars().count() <= 20, "{line:?}");
        }
    }

    #[test]
    fn an_entry_with_nothing_in_it_still_gets_a_row() {
        // A tool call that said nothing happened, and a gap in the list would
        // hide it.
        let out = lines(&[said(Who::Tool, "")], 20);
        assert_eq!(out.len(), 1);
        assert!(out[0].starts_with(Who::Tool.mark()));
    }

    #[test]
    fn a_cjk_line_is_measured_in_columns() {
        // One character, two columns. Measured by length it overruns by one
        // column per glyph.
        let out = lines(&[said(Who::Agent, "读取文件并检查每一行的内容")], 18);
        for line in &out {
            let columns: usize = line.chars().map(super::columns).sum();
            assert!(columns <= 18, "{line:?} is {columns} columns");
        }
    }
}
