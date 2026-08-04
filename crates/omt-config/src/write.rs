//! Writing a value back without destroying the file around it.
//!
//! Editing the concrete syntax tree rather than round-tripping through a value.
//! A round trip is much less code and it silently deletes every comment the
//! user wrote, reorders their keys, and reformats their file — once, invisibly,
//! the first time anything is changed from a settings UI.

use toml_edit::{DocumentMut, Item, Table, Value as TomlValue};

/// What went wrong writing a setting.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WriteError {
    /// The existing file could not be parsed.
    #[error("could not parse the existing config: {0}")]
    Parse(String),
    /// The path runs through something that is not a table.
    #[error("`{key}` cannot be set: `{at}` is not a table")]
    NotATable {
        /// The key being set.
        key: String,
        /// The part of the path that is in the way.
        at: String,
    },
    /// The value could not be represented in TOML.
    #[error("`{0}` cannot be represented in TOML")]
    Unrepresentable(String),
}

/// Set one dotted key in a TOML document, preserving everything else.
///
/// Returns the new document text. Only the targeted value's span is rewritten;
/// comments, key order and formatting survive untouched.
///
/// # Errors
/// Fails if the document does not parse, if the path runs through a non-table,
/// or if the value has no TOML representation.
pub fn set_value(
    existing: &str,
    key: &str,
    value: &serde_json::Value,
) -> Result<String, WriteError> {
    let mut doc: DocumentMut = existing
        .parse()
        .map_err(|e: toml_edit::TomlError| WriteError::Parse(e.to_string()))?;

    let parts: Vec<&str> = key.split('.').collect();
    let Some((leaf, tables)) = parts.split_last() else {
        return Err(WriteError::NotATable {
            key: key.to_owned(),
            at: String::new(),
        });
    };

    let mut item: &mut Item = doc.as_item_mut();
    for (i, part) in tables.iter().enumerate() {
        let entry = item
            .as_table_mut()
            .ok_or_else(|| WriteError::NotATable {
                key: key.to_owned(),
                at: parts[..i].join("."),
            })?
            .entry(part)
            // A missing intermediate table is created implicitly, so setting
            // one key in a file that has never had that section does not
            // require the caller to build the scaffolding first.
            .or_insert_with(|| Item::Table(Table::new()));
        item = entry;
    }

    let table = item.as_table_mut().ok_or_else(|| WriteError::NotATable {
        key: key.to_owned(),
        at: tables.join("."),
    })?;

    let toml_value = to_toml(value).ok_or_else(|| WriteError::Unrepresentable(key.to_owned()))?;

    match table.get_mut(leaf) {
        // Assigning into the existing item keeps its decor — the comment on
        // the line above and the spacing around the `=` are the user's.
        Some(existing_item) => {
            let decor = existing_item.as_value().map(|v| v.decor().clone());
            let mut new_value = toml_value;
            if let Some(d) = decor {
                *new_value.decor_mut() = d;
            }
            *existing_item = Item::Value(new_value);
        }
        None => {
            table.insert(leaf, Item::Value(toml_value));
        }
    }

    Ok(doc.to_string())
}

fn to_toml(value: &serde_json::Value) -> Option<TomlValue> {
    Some(match value {
        serde_json::Value::Bool(b) => TomlValue::from(*b),
        serde_json::Value::String(s) => TomlValue::from(s.as_str()),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                TomlValue::from(i)
            } else {
                TomlValue::from(n.as_f64()?)
            }
        }
        serde_json::Value::Array(items) => {
            let mut arr = toml_edit::Array::new();
            for item in items {
                arr.push(to_toml(item)?);
            }
            TomlValue::Array(arr)
        }
        serde_json::Value::Object(map) => {
            let mut table = toml_edit::InlineTable::new();
            for (k, v) in map {
                table.insert(k, to_toml(v)?);
            }
            TomlValue::InlineTable(table)
        }
        // TOML has no null. Writing an empty string instead would be a
        // different value that happens to look empty.
        serde_json::Value::Null => return None,
    })
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
    fn a_comment_survives_a_write() {
        // The whole reason this edits a syntax tree. A round trip through a
        // value would delete this once, invisibly, the first time anything was
        // changed from a settings UI.
        let before = "\
# my carefully explained choice
[terminal]
scrollback_lines = 10000
";
        let after = set_value(before, "terminal.scrollback_lines", &json!(50000)).expect("write");
        assert!(
            after.contains("# my carefully explained choice"),
            "the comment was destroyed:\n{after}"
        );
        assert!(after.contains("scrollback_lines = 50000"), "{after}");
    }

    #[test]
    fn key_order_survives_a_write() {
        // Reordering somebody's file is a diff they did not ask for.
        let before = "\
[terminal]
zebra = 1
apple = 2
middle = 3
";
        let after = set_value(before, "terminal.apple", &json!(99)).expect("write");
        let order: Vec<&str> = after
            .lines()
            .filter(|l| l.contains('='))
            .map(|l| l.split('=').next().unwrap_or("").trim())
            .collect();
        assert_eq!(order, ["zebra", "apple", "middle"]);
    }

    #[test]
    fn untouched_sections_are_left_exactly_alone() {
        let before = "\
[appearance]
theme   =    'nord'   # odd spacing on purpose

[terminal]
scrollback_lines = 1
";
        let after = set_value(before, "terminal.scrollback_lines", &json!(2)).expect("write");
        assert!(
            after.contains("theme   =    'nord'   # odd spacing on purpose"),
            "another section was reformatted:\n{after}"
        );
    }

    #[test]
    fn a_missing_section_is_created() {
        // So setting one key in a file that has never had that section does not
        // make the caller build the scaffolding first.
        let after = set_value("", "appearance.theme", &json!("nord")).expect("write");
        assert!(after.contains("[appearance]"), "{after}");
        assert!(after.contains("theme = \"nord\""), "{after}");
    }

    #[test]
    fn a_nested_section_is_created() {
        let after = set_value("", "appearance.font.family", &json!("Menlo")).expect("write");
        let reparsed: DocumentMut = after.parse().expect("valid toml");
        assert_eq!(
            reparsed["appearance"]["font"]["family"].as_str(),
            Some("Menlo")
        );
    }

    #[test]
    fn every_scalar_type_round_trips() {
        for (key, value) in [
            ("a.string", json!("text")),
            ("a.integer", json!(42)),
            ("a.float", json!(1.5)),
            ("a.boolean", json!(true)),
            ("a.array", json!([1, 2, 3])),
        ] {
            let out = set_value("", key, &value).expect(key);
            let doc: DocumentMut = out.parse().expect("valid toml");
            let leaf = key.split('.').next_back().expect("leaf");
            assert!(doc["a"].get(leaf).is_some(), "{key} missing from:\n{out}");
        }
    }

    #[test]
    fn null_is_refused_rather_than_written_as_an_empty_string() {
        // TOML has no null, and an empty string is a different value that
        // happens to look empty.
        let err = set_value("", "a.b", &json!(null)).expect_err("must refuse");
        assert!(matches!(err, WriteError::Unrepresentable(_)), "{err:?}");
    }

    #[test]
    fn a_path_through_a_scalar_is_refused_with_the_offending_part_named() {
        // Overwriting the scalar would silently destroy it.
        let before = "[terminal]\nfont = \"Menlo\"\n";
        let err = set_value(before, "terminal.font.size", &json!(12)).expect_err("must refuse");
        let WriteError::NotATable { at, .. } = &err else {
            panic!("{err:?}");
        };
        assert!(at.contains("terminal.font"), "{at}");
    }

    #[test]
    fn a_broken_file_is_reported_rather_than_overwritten() {
        // Rewriting it would destroy whatever the user was in the middle of
        // fixing.
        let err = set_value("[[[not toml", "a.b", &json!(1)).expect_err("must refuse");
        assert!(matches!(err, WriteError::Parse(_)), "{err:?}");
    }

    #[test]
    fn writing_twice_is_stable() {
        // A write that kept reformatting would produce a diff on every save
        // even when nothing changed.
        let once = set_value("[terminal]\nx = 1\n", "terminal.x", &json!(2)).expect("first");
        let twice = set_value(&once, "terminal.x", &json!(2)).expect("second");
        assert_eq!(once, twice);
    }

    #[test]
    fn the_result_is_still_valid_toml() {
        let out = set_value(
            "# top\n[terminal]\na = 1\n",
            "appearance.theme",
            &json!("nord"),
        )
        .expect("write");
        let doc: DocumentMut = out.parse().expect("valid toml");
        assert_eq!(doc["terminal"]["a"].as_integer(), Some(1));
        assert_eq!(doc["appearance"]["theme"].as_str(), Some("nord"));
    }
}
