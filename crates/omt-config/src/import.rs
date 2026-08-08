//! Reading settings people already have.
//!
//! Onboarding is where a terminal loses people: a fresh install that looks
//! nothing like what they were using is one they close. Importing what they
//! already configured is the cheapest thing that fixes that.
//!
//! Every importer reports what it could **not** map. A migration that silently
//! dropped half a config is worse than one that did nothing, because the user
//! believes they have moved and finds out weeks later that a setting they rely
//! on never came across.

use std::collections::BTreeMap;

use serde_json::Value;

/// What an import produced.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Imported {
    /// Settings that mapped, as dotted omt keys.
    pub settings: BTreeMap<String, Value>,
    /// Source keys omt has nowhere to put, with why.
    pub unmapped: Vec<Unmapped>,
}

/// A setting that did not come across.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unmapped {
    /// The source's own key.
    pub key: String,
    /// Why it did not map, in words a user can act on.
    pub reason: &'static str,
}

/// Where a setting came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// A flat `settings.json` of snake_case keys, as several modern terminals
    /// write. Named for the shape rather than for one product: the importer
    /// reads a format, and a format outlives whichever application introduced
    /// it.
    FlatJson,
    /// VS Code's `settings.json`.
    VsCode,
    /// iTerm2's plist, exported as JSON.
    ITerm2,
}

impl Source {
    /// What to call it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::FlatJson => "flat JSON settings",
            Self::VsCode => "VS Code",
            Self::ITerm2 => "iTerm2",
        }
    }
}

/// What one source key becomes.
struct Mapping {
    /// The source's key.
    from: &'static str,
    /// The omt key.
    to: &'static str,
    /// How to convert the value.
    convert: fn(&Value) -> Option<Value>,
}

fn identity(v: &Value) -> Option<Value> {
    Some(v.clone())
}

/// Points to points, but refuse something that is not a number.
fn number(v: &Value) -> Option<Value> {
    v.as_f64().map(Value::from)
}

fn boolean(v: &Value) -> Option<Value> {
    v.as_bool().map(Value::Bool)
}

/// VS Code writes a font stack; a terminal wants the first real family.
fn first_family(v: &Value) -> Option<Value> {
    let text = v.as_str()?;
    let first = text
        .split(',')
        .next()?
        .trim()
        .trim_matches('\'')
        .trim_matches('"');
    (!first.is_empty()).then(|| Value::String(first.to_owned()))
}

/// VS Code's cursor styles and omt's are the same set under different names.
fn cursor_style(v: &Value) -> Option<Value> {
    let name = match v.as_str()? {
        "block" | "block-outline" => "block",
        "line" | "line-thin" => "bar",
        "underline" | "underline-thin" => "underline",
        _ => return None,
    };
    Some(Value::String(name.to_owned()))
}

const FLAT_JSON: &[Mapping] = &[
    Mapping {
        from: "font_size",
        to: "appearance.font.size",
        convert: number,
    },
    Mapping {
        from: "font_family",
        to: "appearance.font.family",
        convert: first_family,
    },
    Mapping {
        from: "line_height",
        to: "appearance.font.line_height",
        convert: number,
    },
    Mapping {
        from: "enable_font_ligatures",
        to: "appearance.font.ligatures",
        convert: boolean,
    },
    Mapping {
        from: "cursor_blink",
        to: "appearance.cursor.blink",
        convert: boolean,
    },
    Mapping {
        from: "theme",
        to: "appearance.theme",
        convert: identity,
    },
];

const VSCODE: &[Mapping] = &[
    Mapping {
        from: "terminal.integrated.fontSize",
        to: "appearance.font.size",
        convert: number,
    },
    Mapping {
        from: "terminal.integrated.fontFamily",
        to: "appearance.font.family",
        convert: first_family,
    },
    Mapping {
        from: "terminal.integrated.lineHeight",
        to: "appearance.font.line_height",
        convert: number,
    },
    Mapping {
        from: "terminal.integrated.letterSpacing",
        to: "appearance.font.letter_spacing",
        convert: number,
    },
    Mapping {
        from: "terminal.integrated.cursorStyle",
        to: "appearance.cursor.style",
        convert: cursor_style,
    },
    Mapping {
        from: "terminal.integrated.cursorBlinking",
        to: "appearance.cursor.blink",
        convert: boolean,
    },
    Mapping {
        from: "terminal.integrated.scrollback",
        to: "terminal.scrollback_lines",
        convert: number,
    },
];

/// Import settings from another terminal or editor.
///
/// Never fails on an unknown key: it reports it. A migration that refused
/// because it met one setting it did not know would be a migration nobody
/// completes.
#[must_use]
pub fn import(source: Source, json: &Value) -> Imported {
    let table = match source {
        Source::FlatJson => FLAT_JSON,
        Source::VsCode | Source::ITerm2 => VSCODE,
    };
    let Some(object) = json.as_object() else {
        return Imported::default();
    };

    let mut out = Imported::default();
    for (key, value) in object {
        // Only what this source is *about*. VS Code's settings file has
        // hundreds of editor keys, and listing every one as "unmapped" would
        // bury the handful the user actually cares about.
        let relevant = match source {
            Source::VsCode => key.starts_with("terminal."),
            _ => true,
        };
        if !relevant {
            continue;
        }

        match table.iter().find(|m| m.from == *key) {
            Some(mapping) => match (mapping.convert)(value) {
                Some(converted) => {
                    out.settings.insert(mapping.to.to_owned(), converted);
                }
                None => out.unmapped.push(Unmapped {
                    key: key.clone(),
                    reason: "the value is not one omt understands",
                }),
            },
            None => out.unmapped.push(Unmapped {
                key: key.clone(),
                reason: "omt has no equivalent setting",
            }),
        }
    }
    out
}

/// Where each source usually keeps its settings.
///
/// Suggestions for a picker, not a search: reading a path a user did not point
/// at is a surprise, however convenient.
#[must_use]
pub fn usual_paths(source: Source) -> Vec<&'static str> {
    match source {
        // The paths this shape is usually found at. A list rather than one
        // entry because the same format appears under several directories and
        // omt reads whichever is there.
        // Where this shape is usually found. Several directories, because the
        // same format is written under more than one, and omt reads whichever
        // exists rather than requiring the user to say.
        Source::FlatJson => vec![
            "~/.config/omt/import/settings.json",
            "~/.terminal-settings.json",
        ],
        Source::VsCode => vec![
            "~/Library/Application Support/Code/User/settings.json",
            "~/.config/Code/User/settings.json",
        ],
        Source::ITerm2 => vec!["~/Library/Preferences/com.googlecode.iterm2.plist"],
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
    use serde_json::json;

    #[test]
    fn flat_json_font_settings_come_across() {
        let imported = import(
            Source::FlatJson,
            &json!({
                "font_size": 14,
                "font_family": "JetBrains Mono",
                "enable_font_ligatures": true,
            }),
        );
        assert_eq!(
            imported.settings.get("appearance.font.size"),
            Some(&json!(14.0))
        );
        assert_eq!(
            imported.settings.get("appearance.font.family"),
            Some(&json!("JetBrains Mono"))
        );
        assert_eq!(
            imported.settings.get("appearance.font.ligatures"),
            Some(&json!(true))
        );
    }

    #[test]
    fn a_vscode_font_stack_becomes_one_family() {
        // VS Code writes a CSS-style stack; a terminal wants the first real
        // family, and passing the whole string through would name a font that
        // does not exist.
        let imported = import(
            Source::VsCode,
            &json!({ "terminal.integrated.fontFamily": "'Fira Code', Menlo, monospace" }),
        );
        assert_eq!(
            imported.settings.get("appearance.font.family"),
            Some(&json!("Fira Code"))
        );
    }

    #[test]
    fn vscode_cursor_styles_map_onto_the_same_set_under_other_names() {
        for (theirs, ours) in [
            ("line", "bar"),
            ("block", "block"),
            ("underline", "underline"),
        ] {
            let imported = import(
                Source::VsCode,
                &json!({ "terminal.integrated.cursorStyle": theirs }),
            );
            assert_eq!(
                imported.settings.get("appearance.cursor.style"),
                Some(&json!(ours)),
                "{theirs}"
            );
        }
    }

    #[test]
    fn what_did_not_map_is_reported_rather_than_dropped() {
        // A migration that silently dropped half a config is worse than one
        // that did nothing: the user believes they have moved and finds out
        // weeks later.
        let imported = import(
            Source::FlatJson,
            &json!({ "font_size": 14, "some_unknown_thing": true }),
        );
        assert_eq!(imported.unmapped.len(), 1);
        assert_eq!(imported.unmapped[0].key, "some_unknown_thing");
        assert!(!imported.unmapped[0].reason.is_empty());
    }

    #[test]
    fn an_unknown_key_does_not_abort_the_import() {
        // A migration that refused because it met one setting it did not know
        // is a migration nobody completes.
        let imported = import(
            Source::FlatJson,
            &json!({ "nonsense": 1, "font_size": 16, "more_nonsense": [1, 2] }),
        );
        assert_eq!(
            imported.settings.get("appearance.font.size"),
            Some(&json!(16.0))
        );
        assert_eq!(imported.unmapped.len(), 2);
    }

    #[test]
    fn vscodes_hundreds_of_editor_keys_are_not_listed_as_unmapped() {
        // Listing every one would bury the handful the user cares about.
        let imported = import(
            Source::VsCode,
            &json!({
                "editor.fontSize": 12,
                "workbench.colorTheme": "Dark+",
                "files.autoSave": "off",
                "terminal.integrated.fontSize": 13,
            }),
        );
        assert!(imported.unmapped.is_empty(), "{:?}", imported.unmapped);
        assert_eq!(
            imported.settings.get("appearance.font.size"),
            Some(&json!(13.0))
        );
    }

    #[test]
    fn a_value_of_the_wrong_type_is_reported_not_coerced() {
        // Coercing "large" to a number would give somebody a font size they
        // never chose.
        let imported = import(Source::FlatJson, &json!({ "font_size": "large" }));
        assert!(imported.settings.is_empty());
        assert_eq!(imported.unmapped[0].key, "font_size");
        assert!(imported.unmapped[0].reason.contains("value"));
    }

    #[test]
    fn an_unrecognised_cursor_style_is_reported_rather_than_guessed() {
        let imported = import(
            Source::VsCode,
            &json!({ "terminal.integrated.cursorStyle": "hexagon" }),
        );
        assert!(imported.settings.is_empty());
        assert_eq!(imported.unmapped.len(), 1);
    }

    #[test]
    fn a_file_that_is_not_an_object_imports_nothing_rather_than_panicking() {
        assert_eq!(
            import(Source::FlatJson, &json!([1, 2, 3])),
            Imported::default()
        );
        assert_eq!(import(Source::VsCode, &json!(null)), Imported::default());
    }

    #[test]
    fn every_source_suggests_where_to_look_rather_than_searching() {
        // Reading a path the user did not point at is a surprise, however
        // convenient.
        for source in [Source::FlatJson, Source::VsCode, Source::ITerm2] {
            assert!(!usual_paths(source).is_empty(), "{source:?}");
            assert!(!source.name().is_empty());
        }
    }

    #[test]
    fn imported_keys_are_ones_the_config_schema_knows() {
        // An import that produced a key the config layer rejects would look
        // like it worked and then be dropped with a diagnostic the user never
        // reads.
        let imported = import(
            Source::VsCode,
            &json!({
                "terminal.integrated.fontSize": 13,
                "terminal.integrated.scrollback": 50_000,
            }),
        );
        for key in imported.settings.keys() {
            assert!(
                key.starts_with("appearance.") || key.starts_with("terminal."),
                "`{key}` is not in a namespace omt has"
            );
        }
    }
}
