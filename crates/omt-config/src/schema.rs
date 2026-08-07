//! What can be configured, declared once.
//!
//! Values are merged from files; this is the other half — what the keys *are*.
//! Without it a setting is discoverable only by already knowing its name, which
//! makes every setting a thing you have to be told about.
//!
//! Declared rather than derived from whatever happens to be in a file: a key
//! nobody has set still exists, and a settings screen that only lists what
//! somebody already changed is not a settings screen.

/// What a setting holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// True or false.
    Bool,
    /// A whole number.
    Integer,
    /// A decimal.
    Number,
    /// Text.
    Text,
    /// One of a fixed set.
    Enumerated,
}

impl Kind {
    /// Its wire spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::Integer => "integer",
            Self::Number => "number",
            Self::Text => "text",
            Self::Enumerated => "enum",
        }
    }
}

/// One thing a user can set.
#[derive(Debug, Clone, Copy)]
pub struct Setting {
    /// Its dotted key.
    pub key: &'static str,
    /// What it holds.
    pub kind: Kind,
    /// What it is when nobody has set it, as JSON text.
    ///
    /// A default for everything, because P11's rule is that a feature works
    /// before it is configured — a setting with no default is a setting that
    /// must be set, which is the same failure with extra steps.
    pub default: &'static str,
    /// One line about what it does.
    pub doc: &'static str,
    /// The permitted values, for an enumerated setting.
    pub choices: &'static [&'static str],
}

/// Every setting omt understands.
///
/// One table. A settings screen, `config.schema` and the validator all read it,
/// so a key cannot exist in one and be missing from another.
pub const SETTINGS: &[Setting] = &[
    Setting {
        key: "font.family",
        kind: Kind::Text,
        default: "\"monospace\"",
        doc: "The font a client renders the terminal in.",
        choices: &[],
    },
    Setting {
        key: "font.size",
        kind: Kind::Number,
        default: "13.0",
        doc: "Point size.",
        choices: &[],
    },
    Setting {
        key: "font.line_height",
        kind: Kind::Number,
        default: "1.2",
        doc: "Line height as a multiple of the font size.",
        choices: &[],
    },
    Setting {
        key: "appearance.theme",
        kind: Kind::Text,
        default: "\"omt dark\"",
        doc: "The theme, by name. Built in, or one imported into the themes directory.",
        choices: &[],
    },
    Setting {
        key: "terminal.scrollback_lines",
        kind: Kind::Integer,
        default: "10000",
        doc: "How many lines of scrollback each session keeps.",
        choices: &[],
    },
    Setting {
        key: "terminal.blocks",
        kind: Kind::Bool,
        default: "true",
        doc: "Track command blocks. On for every shell; a shell that emits no marks still gets blocks, with less known about each.",
        choices: &[],
    },
    Setting {
        key: "terminal.highlight",
        kind: Kind::Bool,
        default: "true",
        doc: "Colour the command line by token.",
        choices: &[],
    },
    Setting {
        key: "hints.enabled",
        kind: Kind::Bool,
        default: "true",
        doc: "Show the hint line. It stops offering the introduction once the prefix has been used.",
        choices: &[],
    },
    Setting {
        key: "server.bind",
        kind: Kind::Text,
        default: "\"127.0.0.1:7717\"",
        doc: "Where the web surface listens. Loopback by default, deliberately.",
        choices: &[],
    },
    Setting {
        key: "agents.detection",
        kind: Kind::Enumerated,
        default: "\"all\"",
        doc: "Which agent-detection sources to run.",
        choices: &["all", "told-only", "off"],
    },
    Setting {
        key: "notifications.when",
        kind: Kind::Enumerated,
        default: "\"needs-you\"",
        doc: "When to notify. `needs-you` is a card waiting on a human, which is the only one worth a buzz by default.",
        choices: &["needs-you", "any-change", "off"],
    },
];

/// One setting by key.
#[must_use]
pub fn setting(key: &str) -> Option<&'static Setting> {
    SETTINGS.iter().find(|s| s.key == key)
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
    fn every_setting_has_a_default() {
        // P11: a feature works before it is configured. A setting with no
        // default is one that must be set, which is the same failure with an
        // extra step in front of it.
        for s in SETTINGS {
            assert!(!s.default.is_empty(), "{} has no default", s.key);
        }
    }

    #[test]
    fn every_default_is_valid_json_of_its_declared_kind() {
        // A default that does not parse is a default nothing can apply, and it
        // would surface as a setting that mysteriously has no value.
        for s in SETTINGS {
            let value: serde_json::Value = serde_json::from_str(s.default)
                .unwrap_or_else(|e| panic!("{} has an unparseable default: {e}", s.key));
            let ok = match s.kind {
                Kind::Bool => value.is_boolean(),
                Kind::Integer => value.is_i64() || value.is_u64(),
                Kind::Number => value.is_number(),
                Kind::Text | Kind::Enumerated => value.is_string(),
            };
            assert!(ok, "{} declares {:?} and defaults to {value}", s.key, s.kind);
        }
    }

    #[test]
    fn an_enumerated_setting_defaults_to_one_of_its_choices() {
        // Otherwise the shipped configuration is invalid against its own
        // schema, which every validator would then report on a fresh install.
        for s in SETTINGS.iter().filter(|s| s.kind == Kind::Enumerated) {
            assert!(!s.choices.is_empty(), "{} lists no choices", s.key);
            let default: String =
                serde_json::from_str(s.default).expect("an enum default is a string");
            assert!(
                s.choices.contains(&default.as_str()),
                "{} defaults to {default}, which is not one of {:?}",
                s.key,
                s.choices
            );
        }
    }

    #[test]
    fn only_enumerated_settings_list_choices() {
        // Choices on a free-text setting suggest a constraint that nothing
        // enforces.
        for s in SETTINGS.iter().filter(|s| s.kind != Kind::Enumerated) {
            assert!(s.choices.is_empty(), "{} lists choices but is not an enum", s.key);
        }
    }

    #[test]
    fn every_setting_says_what_it_does() {
        // A settings screen listing a key with no description is a list of
        // names to guess at.
        for s in SETTINGS {
            assert!(s.doc.len() > 10, "{} has no useful description", s.key);
        }
    }

    #[test]
    fn keys_are_unique_and_dotted() {
        let mut seen = std::collections::BTreeSet::new();
        for s in SETTINGS {
            assert!(seen.insert(s.key), "{} is declared twice", s.key);
            assert!(s.key.contains('.'), "{} is not namespaced", s.key);
        }
    }

    #[test]
    fn a_setting_can_be_found_by_key() {
        assert!(setting("font.size").is_some());
        assert!(setting("font.nonexistent").is_none());
    }
}
