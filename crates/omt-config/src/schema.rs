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
    /// Which layers may set it.
    ///
    /// Not decoration. A repository you cloned must not be able to tell your
    /// daemon what address to bind, so `server.bind` is not settable from a
    /// project file — and the merge is where that is enforced, which is why
    /// this lives on the same table the settings screen reads.
    pub scope: crate::Scope,
}

/// The schema, as the merge wants it.
///
/// Derived rather than written twice. `SETTINGS` says a settings screen,
/// `config.schema` and the validator all read one table — and until this
/// existed the validator read an empty slice, so every key was unknown, no
/// scope was enforced and a typo was silently applied.
#[must_use]
pub fn key_specs() -> Vec<crate::KeySpec> {
    SETTINGS
        .iter()
        .map(|s| crate::KeySpec {
            key: s.key,
            scope: s.scope,
        })
        .collect()
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
        scope: crate::Scope::Any,
    },
    Setting {
        key: "font.size",
        kind: Kind::Number,
        default: "13.0",
        doc: "Point size.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "font.line_height",
        kind: Kind::Number,
        default: "1.2",
        doc: "Line height as a multiple of the font size.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "appearance.theme",
        kind: Kind::Text,
        default: "\"omt dark\"",
        doc: "The theme, by name. Built in, or one imported into the themes directory.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "terminal.scrollback_lines",
        kind: Kind::Integer,
        default: "10000",
        doc: "How many lines of scrollback each session keeps.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "terminal.blocks",
        kind: Kind::Bool,
        default: "true",
        doc: "Track command blocks. On for every shell; a shell that emits no marks still gets blocks, with less known about each.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "terminal.highlight",
        kind: Kind::Bool,
        default: "true",
        doc: "Colour the command line by token.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "hints.enabled",
        kind: Kind::Bool,
        default: "true",
        doc: "Show the hint line. It stops offering the introduction once the prefix has been used.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "server.bind",
        kind: Kind::Text,
        default: "\"127.0.0.1:7717\"",
        doc: "Where the web surface listens. Loopback by default, deliberately.",
        choices: &[],
        // A repository you cloned must not be able to move your daemon off
        // loopback. This is the key where that matters most, and the scope is
        // what stops it.
        scope: crate::Scope::UserOrAbove,
    },
    Setting {
        key: "agents.detection",
        kind: Kind::Enumerated,
        default: "\"all\"",
        doc: "Which agent-detection sources to run.",
        choices: &["all", "told-only", "off"],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "notifications.when",
        kind: Kind::Enumerated,
        default: "\"needs-you\"",
        doc: "When to notify. `needs-you` is a card waiting on a human, which is the only one worth a buzz by default.",
        choices: &["needs-you", "any-change", "off"],
        scope: crate::Scope::Any,
    },
];

/// One setting by key.
#[must_use]
pub fn setting(key: &str) -> Option<&'static Setting> {
    SETTINGS.iter().find(|s| s.key == key)
}

/// A problem found while checking a value against its descriptor.
///
/// `docs/architecture/10-configuration.md` §5.1. Every pass runs to completion
/// collecting these; none aborts on the first problem. Somebody who mistyped
/// four settings wants to be told about four, not to fix one, restart, and
/// find the next — which is four restarts to learn what one run could have
/// said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// Which key.
    pub key: String,
    /// What is wrong.
    pub message: String,
    /// What to do about it.
    pub help: String,
}

/// What kind of thing a JSON value is, in the descriptor's vocabulary.
fn kind_of(value: &serde_json::Value) -> Option<Kind> {
    match value {
        serde_json::Value::Bool(_) => Some(Kind::Bool),
        serde_json::Value::Number(n) => {
            if n.is_f64() && n.as_i64().is_none() {
                Some(Kind::Number)
            } else {
                Some(Kind::Integer)
            }
        }
        serde_json::Value::String(_) => Some(Kind::Text),
        _ => None,
    }
}

/// Whether a value of this kind can be used where that kind is wanted.
///
/// An integer is acceptable where a decimal is wanted, because `1` and `1.0`
/// are the same number and refusing the first would fail a config that is
/// correct in every way somebody can see. The reverse is not true: silently
/// truncating `1.5` to `1` would apply a setting nobody wrote.
const fn accepts(wanted: Kind, found: Kind) -> bool {
    matches!(
        (wanted, found),
        (Kind::Bool, Kind::Bool)
            | (Kind::Integer, Kind::Integer)
            | (Kind::Number, Kind::Number | Kind::Integer)
            | (Kind::Text | Kind::Enumerated, Kind::Text)
    )
}

/// Check flattened configuration values against the descriptors.
///
/// Returns every problem, in key order. Ordered so two runs over the same file
/// report the same thing in the same sequence — a diagnostic list that shuffles
/// is one nobody can diff between runs.
#[must_use]
pub fn validate(values: &std::collections::BTreeMap<String, serde_json::Value>) -> Vec<Problem> {
    let by_key: std::collections::BTreeMap<&str, &Setting> =
        SETTINGS.iter().map(|s| (s.key, s)).collect();
    let mut problems = Vec::new();

    for (key, value) in values {
        let Some(setting) = by_key.get(key.as_str()) else {
            // Unknown keys are the merge pass's business; reporting them twice
            // would have somebody fix one message and still see the other.
            continue;
        };

        let Some(found) = kind_of(value) else {
            problems.push(Problem {
                key: key.clone(),
                message: format!("`{key}` cannot hold a {}", shape_name(value)),
                help: format!("it holds {}", setting.kind.as_str()),
            });
            continue;
        };

        if !accepts(setting.kind, found) {
            problems.push(Problem {
                key: key.clone(),
                message: format!(
                    "`{key}` holds {} but a {} was written",
                    setting.kind.as_str(),
                    found.as_str()
                ),
                help: format!("write a {} value", setting.kind.as_str()),
            });
            continue;
        }

        if setting.kind == Kind::Enumerated
            && let Some(text) = value.as_str()
            && !setting.choices.contains(&text)
        {
            problems.push(Problem {
                key: key.clone(),
                message: format!("`{text}` is not one of the values `{key}` accepts"),
                // The choices, listed. "Invalid value" with the valid ones
                // withheld is a message that sends somebody to the source.
                help: format!("it accepts {}", setting.choices.join(", ")),
            });
        }
    }

    problems
}

/// What to call a value that is not a scalar.
fn shape_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Array(_) => "list",
        serde_json::Value::Object(_) => "table",
        serde_json::Value::Null => "nothing",
        _ => "value",
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
            assert!(
                ok,
                "{} declares {:?} and defaults to {value}",
                s.key, s.kind
            );
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
            assert!(
                s.choices.is_empty(),
                "{} lists choices but is not an enum",
                s.key
            );
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

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod validation_tests {
    use super::*;

    fn values(pairs: &[(&str, serde_json::Value)]) -> std::collections::BTreeMap<String, serde_json::Value> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect()
    }

    fn a_setting_of(kind: Kind) -> &'static Setting {
        SETTINGS
            .iter()
            .find(|s| s.kind == kind)
            .unwrap_or_else(|| panic!("a {kind:?} setting"))
    }

    #[test]
    fn a_correct_configuration_reports_nothing() {
        // A validator that complains about a working config is one people stop
        // reading.
        let setting = a_setting_of(Kind::Bool);
        assert!(validate(&values(&[(setting.key, serde_json::json!(true))])).is_empty());
    }

    #[test]
    fn a_value_of_the_wrong_type_is_reported_with_both_types() {
        // "Type mismatch" alone leaves somebody guessing which way round.
        let setting = a_setting_of(Kind::Bool);
        let found = validate(&values(&[(setting.key, serde_json::json!("yes"))]));
        assert_eq!(found.len(), 1);
        assert!(found[0].message.contains("bool"), "{}", found[0].message);
        assert!(found[0].message.contains("text"), "{}", found[0].message);
    }

    #[test]
    fn an_integer_is_accepted_where_a_decimal_is_wanted() {
        // `1` and `1.0` are the same number, and refusing the first fails a
        // config that is correct in every way somebody can see.
        let Some(setting) = SETTINGS.iter().find(|s| s.kind == Kind::Number) else {
            return;
        };
        assert!(validate(&values(&[(setting.key, serde_json::json!(1))])).is_empty());
    }

    #[test]
    fn a_decimal_is_not_quietly_truncated_into_an_integer() {
        // Truncating `1.5` to `1` applies a setting nobody wrote.
        let setting = a_setting_of(Kind::Integer);
        assert_eq!(validate(&values(&[(setting.key, serde_json::json!(1.5))])).len(), 1);
    }

    #[test]
    fn a_value_outside_an_enumeration_lists_the_ones_that_work() {
        // "Invalid value" with the valid ones withheld is a message that sends
        // somebody to the source.
        let Some(setting) = SETTINGS.iter().find(|s| s.kind == Kind::Enumerated) else {
            return;
        };
        let found = validate(&values(&[(setting.key, serde_json::json!("nonsense"))]));
        assert_eq!(found.len(), 1, "{found:?}");
        for choice in setting.choices {
            assert!(found[0].help.contains(choice), "{}", found[0].help);
        }
    }

    #[test]
    fn a_permitted_enumeration_value_is_accepted() {
        let Some(setting) = SETTINGS.iter().find(|s| s.kind == Kind::Enumerated) else {
            return;
        };
        let choice = setting.choices.first().expect("a choice");
        assert!(validate(&values(&[(setting.key, serde_json::json!(choice))])).is_empty());
    }

    #[test]
    fn a_table_where_a_scalar_belongs_is_named_as_a_table() {
        let setting = a_setting_of(Kind::Bool);
        let found = validate(&values(&[(setting.key, serde_json::json!({ "a": 1 }))]));
        assert_eq!(found.len(), 1);
        assert!(found[0].message.contains("table"), "{}", found[0].message);
    }

    #[test]
    fn every_problem_in_the_file_is_reported_in_one_pass() {
        // Fixing one, restarting, and finding the next is four restarts to
        // learn what one run could have said.
        let a = a_setting_of(Kind::Bool);
        let b = a_setting_of(Kind::Integer);
        let found = validate(&values(&[
            (a.key, serde_json::json!("no")),
            (b.key, serde_json::json!("lots")),
        ]));
        assert_eq!(found.len(), 2, "{found:?}");
    }

    #[test]
    fn the_order_is_stable_between_runs() {
        // A diagnostic list that shuffles is one nobody can diff.
        let a = a_setting_of(Kind::Bool);
        let b = a_setting_of(Kind::Integer);
        let input = values(&[(a.key, serde_json::json!("no")), (b.key, serde_json::json!("lots"))]);
        let once = validate(&input);
        for _ in 0..5 {
            assert_eq!(validate(&input), once);
        }
    }

    #[test]
    fn an_unknown_key_is_left_to_the_merge_pass() {
        // Reporting it twice has somebody fix one message and still see the
        // other.
        assert!(validate(&values(&[("nothing.at.all", serde_json::json!(1))])).is_empty());
    }

    #[test]
    fn every_problem_carries_help() {
        let setting = a_setting_of(Kind::Bool);
        for problem in validate(&values(&[(setting.key, serde_json::json!(7))])) {
            assert!(!problem.help.is_empty(), "{}", problem.key);
        }
    }
}
