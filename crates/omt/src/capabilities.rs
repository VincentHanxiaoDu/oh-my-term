//! The capabilities this binary declares, and the dump codegen reads.

use anyhow::Result;
use omt_catalog::{
    CallContext, CapabilityError, CapabilityHandler, CapabilityRegistry, Decl, Effects, Kind,
    Parity, capability,
};
use omt_types::Role;
use serde::{Deserialize, Serialize};

/// Input to `instance.info`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct InfoIn {}

/// What `instance.info` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct InfoOut {
    /// The build's version.
    pub version: String,
    /// The protocol version it speaks.
    pub proto: u16,
}

capability! {
    /// What this instance is.
    pub struct InstanceInfo;
    input  = InfoIn,
    output = InfoOut,
    decl = Decl {
        name: "instance.info",
        group: "instance",
        verb: "info",
        title: "Instance info",
        aliases: &["version"],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Version and protocol of this instance.",
    },
}

struct InstanceInfoHandler;

impl CapabilityHandler<InstanceInfo> for InstanceInfoHandler {
    fn call(&self, _ctx: &CallContext, _input: InfoIn) -> Result<InfoOut, CapabilityError> {
        Ok(InfoOut {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            proto: omt_proto::PROTO_VERSION,
        })
    }
}

/// Input to `instance.catalog`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct CatalogIn {}

/// One entry of the catalog.
#[derive(Serialize, schemars::JsonSchema)]
pub struct CatalogEntry {
    /// Its dotted name.
    pub name: String,
    /// Its route.
    pub route: String,
    /// Minimum role.
    pub role: Role,
    /// What omt does when it runs.
    pub effects: Effects,
    /// One line about it.
    pub doc: String,
}

/// What `instance.catalog` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct CatalogOut {
    /// Every capability, in name order.
    pub entries: Vec<CatalogEntry>,
}

capability! {
    /// Everything this instance can do.
    pub struct InstanceCatalog;
    input  = CatalogIn,
    output = CatalogOut,
    decl = Decl {
        name: "instance.catalog",
        group: "instance",
        verb: "catalog",
        title: "List capabilities",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Every capability this instance offers.",
    },
}

struct InstanceCatalogHandler;

impl CapabilityHandler<InstanceCatalog> for InstanceCatalogHandler {
    fn call(&self, _ctx: &CallContext, _input: CatalogIn) -> Result<CatalogOut, CapabilityError> {
        Ok(CatalogOut {
            entries: omt_catalog::linked_decls()
                .into_iter()
                .map(|d| CatalogEntry {
                    name: d.name.to_owned(),
                    route: d.route(),
                    role: d.role,
                    effects: d.effects,
                    doc: d.doc.to_owned(),
                })
                .collect(),
        })
    }
}

/// Input to `events.subscribe`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct SubscribeIn {
    /// What to send.
    #[serde(default)]
    pub filter: omt_events::Filter,
}

/// What `events.subscribe` acknowledges.
#[derive(Serialize, schemars::JsonSchema)]
pub struct SubscribeOut {
    /// The subscription's id.
    pub subscription: String,
}

capability! {
    /// Start receiving events.
    pub struct EventsSubscribe;
    input  = SubscribeIn,
    output = SubscribeOut,
    decl = Decl {
        name: "events.subscribe",
        group: "events",
        verb: "subscribe",
        title: "Subscribe to events",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Subscribe to the event stream.",
    },
}

struct EventsSubscribeHandler;

impl CapabilityHandler<EventsSubscribe> for EventsSubscribeHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        _input: SubscribeIn,
    ) -> Result<SubscribeOut, CapabilityError> {
        Ok(SubscribeOut {
            subscription: omt_types::ClientId::new().to_string(),
        })
    }
}

/// Build the registry this binary serves.
///
/// # Errors
/// Fails if a declaration is invalid, duplicated, or has no handler — all at
/// startup, by name, rather than when a caller trips over it.
pub fn registry() -> Result<CapabilityRegistry> {
    let mut r = CapabilityRegistry::new();
    r.register::<InstanceInfo, _>(InstanceInfoHandler)?;
    r.register::<InstanceCatalog, _>(InstanceCatalogHandler)?;
    r.register::<EventsSubscribe, _>(EventsSubscribeHandler)?;
    r.seal()?;
    Ok(r)
}

/// The catalog as codegen consumes it.
///
/// # Errors
/// Fails if the registry cannot be built or the dump cannot be encoded.
pub fn dump() -> Result<String> {
    let registry = registry()?;
    let mut generator = schemars::SchemaGenerator::default();

    let entries: Vec<_> = registry
        .decls()
        .map(|d| {
            serde_json::json!({
                "name": d.name,
                "group": d.group,
                "verb": d.verb,
                "title": d.title,
                "aliases": d.aliases,
                "hidden": d.hidden,
                "kind": d.kind,
                "role": d.role,
                "effects": d.effects,
                "intent": d.intent,
                "parity": d.parity,
                "since": d.since,
                "doc": d.doc,
                "route": d.route(),
            })
        })
        .collect();

    let schemas = serde_json::json!({
        "InfoIn": generator.subschema_for::<InfoIn>(),
        "InfoOut": generator.subschema_for::<InfoOut>(),
        "CatalogIn": generator.subschema_for::<CatalogIn>(),
        "CatalogOut": generator.subschema_for::<CatalogOut>(),
        "SubscribeIn": generator.subschema_for::<SubscribeIn>(),
        "SubscribeOut": generator.subschema_for::<SubscribeOut>(),
    });

    Ok(serde_json::to_string_pretty(&serde_json::json!({
        "version": 1,
        "proto": omt_proto::PROTO_VERSION,
        "capabilities": entries,
        "schemas": schemas,
    }))?)
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
    fn the_registry_seals() {
        // Sealing is strict, so this failing means something is declared with
        // no handler — found here rather than by a caller.
        registry().expect("the registry must seal");
    }

    #[test]
    fn the_dump_is_deterministic() {
        // Codegen output has to be byte-identical run to run, or every build
        // produces a spurious diff.
        assert_eq!(dump().expect("dump"), dump().expect("dump"));
    }

    #[test]
    fn every_declared_capability_reaches_the_dump() {
        let dump: serde_json::Value = serde_json::from_str(&dump().expect("dump")).expect("parse");
        let names: Vec<_> = dump["capabilities"]
            .as_array()
            .expect("capabilities is an array")
            .iter()
            .map(|c| c["name"].as_str().expect("name").to_owned())
            .collect();
        for expected in ["events.subscribe", "instance.catalog", "instance.info"] {
            assert!(
                names.contains(&expected.to_owned()),
                "{expected} missing from {names:?}"
            );
        }
    }

    #[test]
    fn the_dump_is_sorted() {
        let dump: serde_json::Value = serde_json::from_str(&dump().expect("dump")).expect("parse");
        let names: Vec<_> = dump["capabilities"]
            .as_array()
            .expect("array")
            .iter()
            .map(|c| c["name"].as_str().expect("name").to_owned())
            .collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn effects_appear_as_strings_in_the_dump() {
        // The wire form, all the way through to the generated artifact.
        let dump: serde_json::Value = serde_json::from_str(&dump().expect("dump")).expect("parse");
        let effects = &dump["capabilities"][0]["effects"];
        assert!(effects.is_array(), "expected an array, got {effects}");
    }
}
