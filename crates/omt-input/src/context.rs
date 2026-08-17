//! The contexts a binding can ask about, and the predicates over them.
//!
//! `docs/architecture/16-input-and-keymap.md` §4.1 owns the vocabulary and
//! calls itself the authoritative superset, so the names here are that list
//! and nothing else. A `when` predicate naming something outside it is an
//! error rather than a term that happens to be false: a predicate that can
//! never hold is a binding that never fires, and a binding that never fires
//! looks exactly like a keyboard that is broken.

use std::fmt;

/// One context, by the name a `when` predicate spells it with.
///
/// The discriminants are the bit positions of 16 §4.1's `bitflags`, kept the
/// same so the two can be read side by side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u32)]
pub enum Context {
    /// A pane's pty has focus.
    TerminalFocused = 0,
    /// The workspace explorer panel has focus.
    ExplorerFocused = 1,
    /// The command palette is open.
    PaletteOpen = 2,
    /// omt's own find-in-scrollback is active.
    SearchActive = 3,
    /// Scroll and selection mode.
    CopyMode = 4,
    /// An omt-rendered interaction card has focus.
    CardFocused = 5,
    /// The session or agent picker overlay is open.
    PickerOpen = 6,
    /// The settings surface is open.
    SettingsOpen = 7,
    /// A chord is in flight.
    PendingChord = 8,
    /// The pane has a live agent binding.
    AgentBound = 9,
    /// The inner program is full-screen.
    AltScreen = 10,
    /// DECCKM, set by the inner program.
    AppCursorKeys = 11,
    /// The inner program asked for mouse reporting.
    MouseReporting = 12,
    /// The inner program asked for bracketed paste.
    BracketedPaste = 13,
    /// The focused pane is zoomed.
    Zoomed = 14,
    /// This TUI is the local end of `omt ssh`.
    RemoteThinClient = 15,
    /// The surface is the web client.
    Web = 16,
    /// The surface is the terminal UI.
    Tui = 17,
    /// A hint overlay is up.
    HintMode = 18,
    /// The semantic-open action menu has focus.
    OpenMenuFocused = 19,
}

/// Every context, with the word a predicate names it by.
///
/// One table rather than two functions: a name and its variant added in
/// different places is how a vocabulary starts disagreeing with itself.
pub const CONTEXTS: &[(&str, Context)] = &[
    ("terminal_focused", Context::TerminalFocused),
    ("explorer_focused", Context::ExplorerFocused),
    ("palette_open", Context::PaletteOpen),
    ("search_active", Context::SearchActive),
    ("copy_mode", Context::CopyMode),
    ("card_focused", Context::CardFocused),
    ("picker_open", Context::PickerOpen),
    ("settings_open", Context::SettingsOpen),
    ("pending_chord", Context::PendingChord),
    ("agent_bound", Context::AgentBound),
    ("alt_screen", Context::AltScreen),
    ("app_cursor_keys", Context::AppCursorKeys),
    ("mouse_reporting", Context::MouseReporting),
    ("bracketed_paste", Context::BracketedPaste),
    ("zoomed", Context::Zoomed),
    ("remote_thin_client", Context::RemoteThinClient),
    ("web", Context::Web),
    ("tui", Context::Tui),
    ("hint_mode", Context::HintMode),
    ("open_menu_focused", Context::OpenMenuFocused),
];

impl Context {
    /// The context a name spells, if it is one.
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        CONTEXTS
            .iter()
            .find(|(word, _)| *word == name)
            .map(|(_, c)| *c)
    }

    /// The name it is spelled with.
    #[must_use]
    pub fn name(self) -> &'static str {
        CONTEXTS
            .iter()
            .find(|(_, c)| *c == self)
            .map_or("", |(word, _)| *word)
    }

    const fn bit(self) -> u32 {
        1u32 << (self as u32)
    }
}

/// Which contexts hold right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ContextSet(u32);

impl ContextSet {
    /// Nothing holds.
    pub const EMPTY: Self = Self(0);

    /// A set from the contexts that hold.
    #[must_use]
    pub fn of(contexts: &[Context]) -> Self {
        let mut bits = 0;
        for c in contexts {
            bits |= c.bit();
        }
        Self(bits)
    }

    /// Whether one holds.
    #[must_use]
    pub const fn has(self, context: Context) -> bool {
        self.0 & context.bit() != 0
    }

    /// The same set with one context added.
    #[must_use]
    pub const fn with(self, context: Context) -> Self {
        Self(self.0 | context.bit())
    }

    /// The same set with one context removed.
    #[must_use]
    pub const fn without(self, context: Context) -> Self {
        Self(self.0 & !context.bit())
    }
}

/// A `when` expression that could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PredicateError {
    /// A word that is not a context.
    #[error("`{word}` is not a context omt has")]
    UnknownContext {
        /// The word as written.
        word: String,
        /// The nearest name, when one is close enough to be worth offering.
        nearest: Option<&'static str>,
    },
    /// The expression ran out before it was finished.
    #[error("`{0}` ends in the middle of an expression")]
    Truncated(String),
    /// A bracket with no partner.
    #[error("`{0}` has an unclosed `(`")]
    Unclosed(String),
    /// Something left over after a complete expression.
    #[error("`{rest}` is left over after the expression")]
    Trailing {
        /// What was left.
        rest: String,
    },
}

/// A boolean expression over contexts.
///
/// 10 §8.2 gives the shape (`&&`, `||`, `!`, parentheses) and points at 16 §4.1
/// for the vocabulary, so this type owns neither: it owns the *reading* of one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Predicate {
    /// No `when` at all: it always holds, and counts nothing towards
    /// specificity.
    Always,
    /// One context.
    Is(Context),
    /// The negation of one.
    Not(Box<Predicate>),
    /// Both.
    And(Box<Predicate>, Box<Predicate>),
    /// Either.
    Or(Box<Predicate>, Box<Predicate>),
}

impl fmt::Display for Predicate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Always => write!(f, "always"),
            Self::Is(c) => write!(f, "{}", c.name()),
            Self::Not(inner) => write!(f, "!({inner})"),
            Self::And(a, b) => write!(f, "({a} && {b})"),
            Self::Or(a, b) => write!(f, "({a} || {b})"),
        }
    }
}

impl Predicate {
    /// Read one.
    ///
    /// # Errors
    /// Names a word that is not a context, or an expression that does not
    /// finish.
    pub fn parse(text: &str) -> Result<Self, PredicateError> {
        let tokens = tokenize(text);
        if tokens.is_empty() {
            return Ok(Self::Always);
        }
        let mut cursor = 0;
        let predicate = parse_or(&tokens, &mut cursor, text)?;
        if cursor != tokens.len() {
            return Err(PredicateError::Trailing {
                rest: tokens[cursor..].join(" "),
            });
        }
        Ok(predicate)
    }

    /// Whether it holds for these contexts.
    #[must_use]
    pub fn holds(&self, contexts: ContextSet) -> bool {
        match self {
            Self::Always => true,
            Self::Is(c) => contexts.has(*c),
            Self::Not(inner) => !inner.holds(contexts),
            Self::And(a, b) => a.holds(contexts) && b.holds(contexts),
            Self::Or(a, b) => a.holds(contexts) || b.holds(contexts),
        }
    }

    /// How many context terms it names.
    ///
    /// 16 §2.3 rule 3: more terms beats fewer, which is what makes
    /// `explorer_focused && !search_active` win over `explorer_focused`.
    #[must_use]
    pub fn terms(&self) -> u16 {
        match self {
            Self::Always => 0,
            Self::Is(_) => 1,
            Self::Not(inner) => inner.terms(),
            Self::And(a, b) | Self::Or(a, b) => a.terms().saturating_add(b.terms()),
        }
    }
}

/// Split into words, brackets and operators.
///
/// Hand-written because the grammar is five symbols: a dependency here would be
/// a licence to account for and a parser to learn, for less code than this.
fn tokenize(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '(' | ')' | '!' => {
                if !word.is_empty() {
                    out.push(std::mem::take(&mut word));
                }
                out.push(c.to_string());
            }
            '&' | '|' => {
                if !word.is_empty() {
                    out.push(std::mem::take(&mut word));
                }
                // `&&` and `&` both read as and: a user who wrote one `&`
                // meant the operator, and refusing it teaches nothing.
                if chars.peek() == Some(&c) {
                    chars.next();
                }
                out.push(if c == '&' {
                    "&&".to_owned()
                } else {
                    "||".to_owned()
                });
            }
            c if c.is_whitespace() => {
                if !word.is_empty() {
                    out.push(std::mem::take(&mut word));
                }
            }
            c => word.push(c),
        }
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}

fn parse_or(
    tokens: &[String],
    cursor: &mut usize,
    whole: &str,
) -> Result<Predicate, PredicateError> {
    let mut left = parse_and(tokens, cursor, whole)?;
    while tokens.get(*cursor).map(String::as_str) == Some("||") {
        *cursor += 1;
        let right = parse_and(tokens, cursor, whole)?;
        left = Predicate::Or(Box::new(left), Box::new(right));
    }
    Ok(left)
}

fn parse_and(
    tokens: &[String],
    cursor: &mut usize,
    whole: &str,
) -> Result<Predicate, PredicateError> {
    let mut left = parse_unary(tokens, cursor, whole)?;
    while tokens.get(*cursor).map(String::as_str) == Some("&&") {
        *cursor += 1;
        let right = parse_unary(tokens, cursor, whole)?;
        left = Predicate::And(Box::new(left), Box::new(right));
    }
    Ok(left)
}

fn parse_unary(
    tokens: &[String],
    cursor: &mut usize,
    whole: &str,
) -> Result<Predicate, PredicateError> {
    match tokens.get(*cursor).map(String::as_str) {
        None => Err(PredicateError::Truncated(whole.to_owned())),
        Some("!") => {
            *cursor += 1;
            Ok(Predicate::Not(Box::new(parse_unary(
                tokens, cursor, whole,
            )?)))
        }
        Some("(") => {
            *cursor += 1;
            let inner = parse_or(tokens, cursor, whole)?;
            if tokens.get(*cursor).map(String::as_str) != Some(")") {
                return Err(PredicateError::Unclosed(whole.to_owned()));
            }
            *cursor += 1;
            Ok(inner)
        }
        Some(")" | "&&" | "||") => Err(PredicateError::Truncated(whole.to_owned())),
        Some(word) => {
            *cursor += 1;
            Context::named(word)
                .map(Predicate::Is)
                .ok_or_else(|| PredicateError::UnknownContext {
                    word: word.to_owned(),
                    nearest: nearest_context(word),
                })
        }
    }
}

/// The closest context name, when one is close enough to suggest.
///
/// The usual cause of an unknown name is a near miss, and the usual fix is one
/// character. A suggestion further away than two edits is worse than none:
/// somebody takes it.
fn nearest_context(wanted: &str) -> Option<&'static str> {
    CONTEXTS
        .iter()
        .map(|(name, _)| (distance(wanted, name), *name))
        .filter(|(d, _)| *d <= 2)
        .min_by_key(|(d, _)| *d)
        .map(|(_, name)| name)
}

fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut current = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            current.push(
                (current[j] + 1)
                    .min(previous[j + 1] + 1)
                    .min(previous[j] + cost),
            );
        }
        previous = current;
    }
    previous[b.len()]
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::{Context, ContextSet, Predicate, PredicateError};

    fn holds(expression: &str, contexts: &[Context]) -> bool {
        Predicate::parse(expression)
            .expect("the expression parses")
            .holds(ContextSet::of(contexts))
    }

    #[test]
    fn and_binds_tighter_than_or() {
        // `a || b && c` is `a || (b && c)`. Read the other way, a binding
        // scoped to one surface would fire on another — the kind of wrong that
        // only shows up in somebody else's terminal.
        assert!(holds(
            "copy_mode || palette_open && search_active",
            &[Context::CopyMode]
        ));
        assert!(!holds(
            "copy_mode || palette_open && search_active",
            &[Context::PaletteOpen]
        ));
    }

    #[test]
    fn parentheses_override_precedence() {
        assert!(holds(
            "(copy_mode || palette_open) && !search_active",
            &[Context::PaletteOpen]
        ));
        assert!(!holds(
            "(copy_mode || palette_open) && !search_active",
            &[Context::PaletteOpen, Context::SearchActive]
        ));
    }

    #[test]
    fn an_absent_when_always_holds_and_counts_nothing() {
        let always = Predicate::parse("").expect("empty is Always");
        assert_eq!(always, Predicate::Always);
        assert!(always.holds(ContextSet::EMPTY));
        assert_eq!(always.terms(), 0);
    }

    #[test]
    fn specificity_counts_terms_including_negated_ones() {
        // 16 §2.3 rule 3's own example.
        let two = Predicate::parse("explorer_focused && !search_active").expect("parses");
        let one = Predicate::parse("explorer_focused").expect("parses");
        assert_eq!((two.terms(), one.terms()), (2, 1));
    }

    #[test]
    fn an_unknown_name_is_an_error_with_the_nearest_offered() {
        let err = Predicate::parse("copy_mod").expect_err("not a context");
        match err {
            PredicateError::UnknownContext { word, nearest } => {
                assert_eq!(word, "copy_mod");
                assert_eq!(nearest, Some("copy_mode"));
            }
            other => panic!("wrong error: {other}"),
        }
    }

    #[test]
    fn an_unfinished_expression_is_an_error_rather_than_a_guess() {
        assert!(matches!(
            Predicate::parse("copy_mode &&"),
            Err(PredicateError::Truncated(_))
        ));
        assert!(matches!(
            Predicate::parse("(copy_mode"),
            Err(PredicateError::Unclosed(_))
        ));
        assert!(matches!(
            Predicate::parse("copy_mode palette_open"),
            Err(PredicateError::Trailing { .. })
        ));
    }

    #[test]
    fn every_context_in_the_document_has_a_name_that_round_trips() {
        for (name, context) in super::CONTEXTS {
            assert_eq!(Context::named(name), Some(*context));
            assert_eq!(context.name(), *name);
            assert!(ContextSet::of(&[*context]).has(*context));
            assert!(!ContextSet::EMPTY.has(*context));
        }
    }

    #[test]
    fn the_set_is_the_documented_one_and_no_larger() {
        // 16 §4.1 calls itself the authoritative superset. A name here that is
        // not there is a `when` predicate nobody can look up.
        assert_eq!(super::CONTEXTS.len(), 20);
    }
}
