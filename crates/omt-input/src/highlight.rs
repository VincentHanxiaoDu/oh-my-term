//! Classifying a command line.
//!
//! What this is for is colour, and what it is deliberately not is a shell
//! parser. Reimplementing `bash` grammar to decide what is blue would be an
//! enormous amount of code to get subtly wrong, and being subtly wrong about
//! colour is worse than being coarse about it.
//!
//! It does track quoting, because a flag inside a string highlighted as a flag
//! is the error people notice within a second — and because the same table
//! drives completion later. Two tables would drift, and the drift shows as a
//! token highlighted as a command that the completer does not believe is one.

/// What a token is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// The program being run.
    Command,
    /// A word after the command that is not a flag — `test` in `cargo test`.
    Subcommand,
    /// `-x` or `--long`.
    Flag,
    /// Anything else the command was given.
    Argument,
    /// A quoted run, including its quotes.
    Text,
    /// `$VAR` or `${VAR}`.
    Variable,
    /// `|`, `&&`, `;`, `>`, and friends.
    Operator,
}

/// One classified run of the line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// Byte offset where it starts.
    pub start: usize,
    /// Byte offset after it ends.
    pub end: usize,
    /// What it is.
    pub class: Class,
}

/// The operators that start a new command.
///
/// After any of these the next word is a command again — `ls | grep foo` has
/// two commands, and highlighting `grep` as an argument is the kind of small
/// wrongness that makes the whole feature feel unreliable.
const SEPARATORS: &[&str] = &["|", "||", "&&", ";", "&", "|&"];

/// Classify a command line.
///
/// Byte offsets, so a caller can slice the original without re-encoding, and
/// so a multi-byte character cannot be split by a span boundary.
#[must_use]
pub fn classify(line: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0usize;
    // Whether the next word is the command. True at the start and after every
    // separator.
    let mut expect_command = true;
    // How many words have been classified on this segment. A subcommand is the
    // word *immediately* after the command — `git commit` — and nothing later.
    // Scanning for "no subcommand yet" instead would make `echo "x" tail` call
    // `tail` a subcommand, because the quoted run is not a word.
    let mut words = 0usize;

    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }

        let start = i;
        let class = match bytes[i] {
            // A quoted run, taken whole. Everything inside is text, which is
            // the point: a `--flag` inside quotes is an argument's contents,
            // not a flag.
            q @ (b'"' | b'\'') => {
                i += 1;
                while i < bytes.len() && bytes[i] != q {
                    // A backslash escapes the next byte, including the closing
                    // quote — otherwise `"a\""` ends early and everything after
                    // it is classified as if it were outside the string.
                    if bytes[i] == b'\\' && q == b'"' && i + 1 < bytes.len() {
                        i += 1;
                    }
                    i += 1;
                }
                // Past the closing quote, when there is one. An unterminated
                // string runs to the end of the line, which is what the shell
                // would do with it too.
                i = (i + 1).min(bytes.len());
                // A quoted run is a word: `echo "x" tail` has `tail` as an
                // argument, not as a subcommand that happens to be third.
                if !expect_command {
                    words += 1;
                }
                Class::Text
            }
            b'$' => {
                i += 1;
                if i < bytes.len() && bytes[i] == b'{' {
                    while i < bytes.len() && bytes[i] != b'}' {
                        i += 1;
                    }
                    i = (i + 1).min(bytes.len());
                } else {
                    while i < bytes.len()
                        && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_')
                    {
                        i += 1;
                    }
                }
                Class::Variable
            }
            _ => {
                while i < bytes.len()
                    && !bytes[i].is_ascii_whitespace()
                    && !matches!(bytes[i], b'"' | b'\'')
                {
                    i += 1;
                }
                let word = &line[start..i];
                if SEPARATORS.contains(&word) || is_redirect(word) {
                    expect_command = true;
                    words = 0;
                    Class::Operator
                } else if expect_command {
                    expect_command = false;
                    words = 1;
                    Class::Command
                } else if word.starts_with('-') {
                    // Flags do not consume the subcommand slot: `git -c k=v
                    // commit` still has `commit` as its subcommand.
                    Class::Flag
                } else if words == 1 {
                    words += 1;
                    Class::Subcommand
                } else {
                    words += 1;
                    Class::Argument
                }
            }
        };
        spans.push(Span {
            start,
            end: i,
            class,
        });
    }
    spans
}

/// Whether a word is a redirection.
fn is_redirect(word: &str) -> bool {
    matches!(word, ">" | ">>" | "<" | "2>" | "2>&1" | "&>" | ">&")
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "in a test, expect() is the assertion")]
mod tests {
    use super::*;

    fn classes(line: &str) -> Vec<(Class, &str)> {
        classify(line)
            .into_iter()
            .map(|s| (s.class, &line[s.start..s.end]))
            .collect()
    }

    #[test]
    fn a_command_line_is_classified_by_position() {
        assert_eq!(
            classes("cargo test --workspace crates/omt"),
            vec![
                (Class::Command, "cargo"),
                (Class::Subcommand, "test"),
                (Class::Flag, "--workspace"),
                (Class::Argument, "crates/omt"),
            ]
        );
    }

    #[test]
    fn a_flag_inside_a_string_is_part_of_the_string() {
        // The error people notice within a second.
        let out = classes(r#"echo "--not-a-flag""#);
        assert_eq!(out[1], (Class::Text, r#""--not-a-flag""#));
    }

    #[test]
    fn an_escaped_quote_does_not_end_the_string() {
        // Otherwise everything after it is classified as if it were outside,
        // and the rest of the line is coloured wrong.
        let out = classes(r#"echo "a\"b" tail"#);
        assert_eq!(out[1].0, Class::Text);
        assert_eq!(out[2], (Class::Argument, "tail"));
    }

    #[test]
    fn an_unterminated_string_runs_to_the_end_rather_than_panicking() {
        // Which is what the shell does with it too, and this is called on
        // every keystroke — half a string is the normal state while typing.
        let out = classes(r#"echo "half"#);
        assert_eq!(out.len(), 2);
        assert_eq!(out[1].0, Class::Text);
    }

    #[test]
    fn the_word_after_a_pipe_is_a_command_again() {
        // `ls | grep foo` has two commands. Colouring `grep` as an argument is
        // the kind of small wrongness that makes the feature feel unreliable.
        assert_eq!(
            classes("ls | grep foo"),
            vec![
                (Class::Command, "ls"),
                (Class::Operator, "|"),
                (Class::Command, "grep"),
                (Class::Subcommand, "foo"),
            ]
        );
    }

    #[test]
    fn a_redirection_is_an_operator() {
        let out = classes("cat a.txt > b.txt");
        assert_eq!(out[2].0, Class::Operator);
    }

    #[test]
    fn a_variable_is_its_own_class_in_both_spellings() {
        assert_eq!(classes("echo $HOME")[1].0, Class::Variable);
        assert_eq!(classes("echo ${HOME}")[1], (Class::Variable, "${HOME}"));
    }

    #[test]
    fn only_the_first_bare_word_is_a_subcommand() {
        // `git commit -m message` has one subcommand. Two would be wrong in a
        // way people notice.
        let out = classes("git commit -m message");
        assert_eq!(out[1].0, Class::Subcommand);
        assert_eq!(out[3].0, Class::Argument);
    }

    #[test]
    fn an_empty_line_classifies_to_nothing() {
        assert!(classify("").is_empty());
        assert!(classify("   ").is_empty());
    }

    #[test]
    fn spans_never_split_a_multibyte_character() {
        // Byte offsets are only safe if they land on boundaries. Slicing a
        // string at the middle of a character panics.
        let line = "echo 日本語 --flag";
        for span in classify(line) {
            assert!(line.is_char_boundary(span.start), "{span:?}");
            assert!(line.is_char_boundary(span.end), "{span:?}");
        }
    }

    #[test]
    fn spans_cover_the_line_without_overlapping() {
        // Overlapping spans paint one token in two colours, which reads as a
        // rendering bug.
        let line = "a b | c --d 'e' $F";
        let spans = classify(line);
        for pair in spans.windows(2) {
            assert!(pair[0].end <= pair[1].start, "{pair:?}");
        }
    }
}
