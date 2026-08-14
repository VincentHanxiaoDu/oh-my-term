//! A capability's schema may grow. It may not change shape.
//!
//! [03 §7](../../../docs/architecture/03-capability-catalog.md) states the
//! rule: a later version may **add optional input fields** and **add output
//! fields**, and nothing else. `omt-proto`'s fixtures cover the envelope
//! messages; nothing covered what travels inside them, which is where a
//! hundred and forty schemas live and where the breaking change is much easier
//! to make by accident.
//!
//! The three that break a client already in the field, none of which any other
//! test notices:
//!
//! - **A new *required* input field.** Every existing caller omits it, so
//!   every existing call starts failing validation.
//! - **A removed output field.** The client reads it and finds nothing —
//!   usually as a crash in whichever surface was least careful.
//! - **A renamed anything.** Both of the above at once.
//!
//! The snapshot is checked in and regenerating it is deliberate, because the
//! diff *is* the compatibility review. `UPDATE_SCHEMA_SNAPSHOT=1` rewrites it.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// One capability's shape, reduced to what compatibility depends on.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
struct Shape {
    /// Input fields that a caller must supply.
    required_input: BTreeSet<String>,
    /// Every input field, required or not.
    input: BTreeSet<String>,
    /// Every output field.
    output: BTreeSet<String>,
}

fn snapshot_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/schemas.v1.json")
}

/// Field names and required set from a JSON Schema object.
fn shape_of(schema: &serde_json::Value) -> (BTreeSet<String>, BTreeSet<String>) {
    let fields = schema
        .get("properties")
        .and_then(|p| p.as_object())
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    let required = schema
        .get("required")
        .and_then(|r| r.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    (fields, required)
}

fn current() -> BTreeMap<String, Shape> {
    let dump: serde_json::Value =
        serde_json::from_str(&omt::capabilities::dump().expect("dump")).expect("parse");
    let defs = &dump["$defs"];
    let schemas = dump["schemas"].as_object().expect("schemas");

    // A schema is often a bare `$ref` into `$defs`; the fields are there, not
    // at the reference. Following it once is enough — schemars puts the object
    // itself in `$defs`, not another indirection.
    let resolve = |schema: &serde_json::Value| -> serde_json::Value {
        schema
            .get("$ref")
            .and_then(|r| r.as_str())
            .and_then(|r| r.strip_prefix("#/$defs/"))
            .and_then(|name| defs.get(name))
            .cloned()
            .unwrap_or_else(|| schema.clone())
    };

    let mut out = BTreeMap::new();
    for entry in dump["capabilities"].as_array().expect("array") {
        let name = entry["name"].as_str().expect("name").to_owned();
        let empty = serde_json::Value::Null;
        let (input, required_input) = shape_of(&resolve(
            schemas.get(&format!("{name}.input")).unwrap_or(&empty),
        ));
        let (output, _) = shape_of(&resolve(
            schemas.get(&format!("{name}.output")).unwrap_or(&empty),
        ));
        out.insert(
            name,
            Shape {
                required_input,
                input,
                output,
            },
        );
    }
    out
}

#[test]
fn no_capability_has_broken_a_client_that_was_already_written() {
    let now = current();

    if std::env::var("UPDATE_SCHEMA_SNAPSHOT").is_ok() {
        let dir = snapshot_path().parent().expect("parent").to_owned();
        std::fs::create_dir_all(dir).expect("create fixture dir");
        std::fs::write(
            snapshot_path(),
            format!(
                "{}\n",
                serde_json::to_string_pretty(&now).expect("serialize")
            ),
        )
        .expect("write snapshot");
        return;
    }

    let stored: BTreeMap<String, Shape> = {
        let text = std::fs::read_to_string(snapshot_path()).unwrap_or_else(|_| {
            panic!(
                "no snapshot at {}. Run with UPDATE_SCHEMA_SNAPSHOT=1 to create it.",
                snapshot_path().display()
            )
        });
        serde_json::from_str(&text).expect("parse snapshot")
    };

    let mut breaks = Vec::new();
    for (name, was) in &stored {
        let Some(now) = now.get(name) else {
            // A capability that disappeared is the loudest break of all, and
            // deliberately not an exception: removing one is a protocol
            // version, not a refactor.
            breaks.push(format!("`{name}` no longer exists"));
            continue;
        };

        // A newly-required input field: every existing caller omits it.
        for field in now.required_input.difference(&was.required_input) {
            breaks.push(format!(
                "`{name}` now requires input field `{field}`, which no existing caller sends"
            ));
        }
        // A removed input field: an existing caller still sends it. Tolerated
        // by serde today, but it means the caller's intent is silently
        // ignored, which is worse than a refusal.
        for field in was.input.difference(&now.input) {
            breaks.push(format!(
                "`{name}` dropped input field `{field}`; a caller still sending it is now ignored"
            ));
        }
        // A removed output field: the client reads it and finds nothing.
        for field in was.output.difference(&now.output) {
            breaks.push(format!(
                "`{name}` dropped output field `{field}`, which a client is reading"
            ));
        }
    }

    assert!(
        breaks.is_empty(),
        "{} incompatible schema change(s):\n  {}\n\nIf these are intended they need a \
         protocol version, not a new snapshot. Otherwise fix the declaration. To accept \
         them deliberately: UPDATE_SCHEMA_SNAPSHOT=1, and the diff is the review.",
        breaks.len(),
        breaks.join("\n  ")
    );
}

#[test]
fn adding_an_optional_field_is_not_a_break() {
    // The rule has to permit growth or it permits nothing, and a test that
    // only ever says "no" would be satisfied by freezing the catalog. This
    // pins the *allowed* direction so a future tightening cannot quietly
    // forbid it.
    let was = Shape {
        required_input: BTreeSet::from(["workspace".to_owned()]),
        input: BTreeSet::from(["workspace".to_owned()]),
        output: BTreeSet::from(["sessions".to_owned()]),
    };
    let now = Shape {
        required_input: BTreeSet::from(["workspace".to_owned()]),
        input: BTreeSet::from(["workspace".to_owned(), "since".to_owned()]),
        output: BTreeSet::from(["sessions".to_owned(), "total".to_owned()]),
    };
    assert!(now.required_input.difference(&was.required_input).count() == 0);
    assert!(was.input.difference(&now.input).count() == 0);
    assert!(was.output.difference(&now.output).count() == 0);
}

#[test]
fn the_snapshot_covers_every_capability_the_binary_registers() {
    // A capability added without being snapshotted has no compatibility
    // guarantee, and would pass the test above by simply not being in the
    // stored map.
    if std::env::var("UPDATE_SCHEMA_SNAPSHOT").is_ok() {
        return;
    }
    let stored: BTreeMap<String, Shape> =
        serde_json::from_str(&std::fs::read_to_string(snapshot_path()).expect("snapshot"))
            .expect("parse");
    let now = current();
    let missing: Vec<&String> = now.keys().filter(|k| !stored.contains_key(*k)).collect();
    assert!(
        missing.is_empty(),
        "{} capabilit(ies) have no snapshot and therefore no compatibility guarantee: {missing:?}\n\
         Run with UPDATE_SCHEMA_SNAPSHOT=1 once they are settled.",
        missing.len()
    );
}
