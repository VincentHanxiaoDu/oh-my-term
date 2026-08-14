//! A checked-in example of every message, and the guarantee it buys.
//!
//! P5: a client built against an older omt keeps working. The wire format is
//! the one thing in this project that cannot be fixed by shipping a new
//! version, because the whole point is that the other end is *not* the new
//! version — a phone that has not updated, a `omt-hook` binary installed
//! months ago into an agent's config, a thin client on the far side of an ssh
//! connection that omt did not install.
//!
//! So each message has a fixture on disk, and two tests:
//!
//! - **The fixture still parses.** This is the compatibility claim. A field
//!   renamed, a tag changed, a required field added — each is a change that
//!   compiles, regenerates the schema, passes every other test, and silently
//!   stops understanding the client that is already out there. Only a frozen
//!   example catches it.
//! - **What we serialize today still round-trips.** This is the weaker,
//!   ordinary claim, and it is the one that would have passed either way.
//!
//! Regenerating a fixture is therefore a deliberate act. `UPDATE_FIXTURES=1`
//! rewrites them, and the diff that produces is the change being reviewed.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]

use std::path::PathBuf;

use omt_proto::{
    Call, CallOutcome, CallResult, Goodbye, GoodbyeReason, Hello, ProtoMessage, Resync, Subscribe,
    Welcome,
};
use omt_types::Role;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Every message, with one worked example each.
///
/// Listed rather than derived: a variant added to `ProtoMessage` and forgotten
/// here would be a message with no compatibility guarantee, and the test below
/// counts them so that cannot happen quietly.
fn examples() -> Vec<(&'static str, ProtoMessage)> {
    let request = omt_catalog::RequestId {
        device: omt_types::DeviceId::from_uuid(uuid::Uuid::nil()),
        n: 7,
    };
    vec![
        (
            "hello",
            ProtoMessage::Hello(Hello {
                proto: 1,
                client: "omt-web/0.1.0".to_owned(),
                token: Some("omt_c_example".to_owned()),
            }),
        ),
        (
            "welcome",
            ProtoMessage::Welcome(Welcome {
                proto: 1,
                role: Role::Operator,
                catalog_hash: "0123456789abcdef".to_owned(),
                capabilities: vec!["instance.info".to_owned(), "session.list".to_owned()],
                auth_methods: vec!["bearer".to_owned()],
            }),
        ),
        (
            "goodbye",
            ProtoMessage::Goodbye(Goodbye {
                reason: GoodbyeReason::ShuttingDown,
                detail: "the instance is stopping".to_owned(),
            }),
        ),
        (
            "call",
            ProtoMessage::Call(Call {
                request,
                capability: "session.list".to_owned(),
                input: serde_json::json!({ "workspace": "ws_1" }),
                intent: None,
            }),
        ),
        (
            "result",
            ProtoMessage::Result(CallResult {
                request,
                outcome: CallOutcome::Ok {
                    output: serde_json::json!({ "sessions": [] }),
                },
            }),
        ),
        (
            "subscribe",
            ProtoMessage::Subscribe(Subscribe {
                filter: omt_events::Filter::default(),
                since: std::collections::BTreeMap::from([(
                    "instance".to_owned(),
                    omt_types::Seq::new(12),
                )]),
            }),
        ),
        (
            "resync",
            ProtoMessage::Resync(Resync {
                scope_key: "instance".to_owned(),
                reason: omt_events::LagReason::WindowExceeded,
                dropped: 3,
                from: omt_types::Seq::new(42),
            }),
        ),
    ]
}

fn check(name: &str, message: &ProtoMessage) {
    let path = fixture_dir().join(format!("{name}.json"));
    let current = serde_json::to_string_pretty(message).expect("serialize");

    if std::env::var("UPDATE_FIXTURES").is_ok() {
        std::fs::create_dir_all(fixture_dir()).expect("create fixture dir");
        std::fs::write(&path, format!("{current}\n")).expect("write fixture");
        return;
    }

    let stored = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "no fixture at {}. A message with no fixture has no compatibility \
             guarantee. Run with UPDATE_FIXTURES=1 to create it, and review the diff.",
            path.display()
        )
    });

    // The compatibility claim: what was on the wire before still parses now.
    let parsed: ProtoMessage = serde_json::from_str(&stored).unwrap_or_else(|e| {
        panic!(
            "the stored `{name}` no longer parses: {e}\n\
             A client built against that shape is now talking to an omt that \
             cannot understand it. If the change is intended, it needs a \
             protocol version, not a new fixture."
        )
    });
    assert_eq!(
        &parsed, message,
        "the stored `{name}` parses but means something different now"
    );
}

#[test]
fn every_message_has_a_fixture_that_still_parses() {
    for (name, message) in examples() {
        check(name, &message);
    }
}

#[test]
fn every_message_round_trips_through_its_own_encoding() {
    // The ordinary claim, which would pass whatever the fixtures said. Kept
    // because it localizes a failure: this one failing means the encoding is
    // broken, and only the fixture test failing means it *changed*.
    for (name, message) in examples() {
        let json = serde_json::to_string(&message).expect("serialize");
        let back: ProtoMessage = serde_json::from_str(&json)
            .unwrap_or_else(|e| panic!("`{name}` did not round trip: {e}\n{json}"));
        assert_eq!(back, message, "`{name}`");
    }
}

#[test]
fn every_message_is_tagged_so_a_reader_can_discriminate_it() {
    // Untagged, a reader would have to guess from the fields present — and two
    // messages that happen to share a shape become indistinguishable the day
    // one of them gains a field.
    for (name, message) in examples() {
        let value: serde_json::Value = serde_json::to_value(&message).expect("serialize");
        assert!(
            value.get("t").and_then(|t| t.as_str()).is_some(),
            "`{name}` has no discriminant: {value}"
        );
    }
}

#[test]
fn a_fixture_exists_for_every_variant_that_can_be_constructed() {
    // The list in `examples` is written by hand, so this is what stops a new
    // variant from shipping with no compatibility guarantee. It counts the
    // variants the enum actually has, by asking the schema rather than by
    // trusting a second hand-written number.
    let schema = schemars::schema_for!(ProtoMessage);
    let json = serde_json::to_value(&schema).expect("schema");
    let variants = json
        .get("oneOf")
        .and_then(|v| v.as_array())
        .map_or(0, Vec::len);
    // Hook messages travel the same path but carry an agent's payload, and
    // their own fixtures live with the hook tests where the payload shapes
    // are. Join messages carry a credential and are covered by the identity
    // suite for the same reason. `Event` carries an `omt-events` envelope
    // whose own fixtures belong with that crate's types.
    let covered_elsewhere = 5;
    assert_eq!(
        examples().len() + covered_elsewhere,
        variants,
        "the enum has {variants} variants and {} are exercised here; a variant \
         with no fixture has no compatibility guarantee",
        examples().len()
    );
}
