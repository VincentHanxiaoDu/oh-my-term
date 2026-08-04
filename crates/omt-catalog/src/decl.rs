//! The declaration-level types: what a `capability!` carries.

use std::fmt;

use bitflags::bitflags;
use omt_types::Role;
use serde::{Deserialize, Serialize};

bitflags! {
    /// What **omt** does when a capability runs.
    ///
    /// Not what an agent's tool does. omt adds no policy over an agent's own
    /// permission gate, so these describe omt's own operation only: they drive
    /// UI affordances and the audit log, never an authorization decision.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct Effects: u8 {
        /// Writes bytes to a session's PTY.
        const WRITES_PTY      = 1 << 0;
        /// Starts a process.
        const SPAWNS_PROCESS  = 1 << 1;
        /// Reads the filesystem.
        const READS_FS        = 1 << 2;
        /// Writes the filesystem.
        const WRITES_FS       = 1 << 3;
        /// Makes an outbound network connection.
        const NETWORK         = 1 << 4;
        /// Destroys something a user would miss.
        const DESTRUCTIVE     = 1 << 5;
    }
}

impl Effects {
    /// The closed set, in wire order.
    const NAMED: [(Self, &'static str); 6] = [
        (Self::DESTRUCTIVE, "destructive"),
        (Self::NETWORK, "network"),
        (Self::READS_FS, "reads_fs"),
        (Self::SPAWNS_PROCESS, "spawns_process"),
        (Self::WRITES_FS, "writes_fs"),
        (Self::WRITES_PTY, "writes_pty"),
    ];

    /// The wire form: a sorted array of lower-snake strings.
    ///
    /// Deliberately not the integer. A bitmask in a schema, an audit log and a
    /// TypeScript client is unreadable at exactly the moments it matters —
    /// reviewing what a capability did — and it silently reinterprets if a flag
    /// is ever renumbered. Sorted so the encoding is canonical.
    #[must_use]
    pub fn to_wire(self) -> Vec<&'static str> {
        Self::NAMED
            .iter()
            .filter(|(flag, _)| self.contains(*flag))
            .map(|(_, name)| *name)
            .collect()
    }

    /// Parse the wire form.
    ///
    /// # Errors
    /// Returns the offending string if it is not one of the closed set. An
    /// unknown effect is refused rather than ignored: silently dropping one
    /// would understate what a capability does.
    pub fn from_wire<'a>(names: impl IntoIterator<Item = &'a str>) -> Result<Self, String> {
        let mut out = Self::empty();
        for n in names {
            let found = Self::NAMED
                .iter()
                .find(|(_, name)| *name == n)
                .ok_or_else(|| n.to_owned())?;
            out |= found.0;
        }
        Ok(out)
    }
}

impl Serialize for Effects {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.to_wire().serialize(s)
    }
}

impl<'de> Deserialize<'de> for Effects {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let names = Vec::<String>::deserialize(d)?;
        Self::from_wire(names.iter().map(String::as_str))
            .map_err(|bad| serde::de::Error::custom(format!("unknown effect `{bad}`")))
    }
}

impl schemars::JsonSchema for Effects {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Effects".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let names: Vec<_> = Self::NAMED.iter().map(|(_, n)| *n).collect();
        schemars::json_schema!({
            "type": "array",
            "items": { "type": "string", "enum": names },
            "uniqueItems": true,
        })
    }
}

/// Whether a capability reads or mutates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Reads. Cacheable and safe to retry by construction.
    Query,
    /// Mutates. Retry safety comes from its [`Intent`] class.
    Command,
}

/// How omt may key deduplication for an append-shaped command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DedupKey {
    /// By the client's intent id alone.
    IntentId,
    /// By intent id and the target it appends to.
    IntentIdAndTarget,
}

/// What makes a repeat of this command safe, or unsafe.
///
/// Five classes, because a retry means something different for each, and the
/// design's worst failure would be treating them as one. Declared per command
/// so dispatch enforces it centrally rather than each transport and client
/// inventing its own answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "class", rename_all = "snake_case")]
pub enum Intent {
    /// Compare-and-swap against a version or a state, plus the caller's
    /// identity and intent id. A repeat returns the original result.
    Cas,
    /// Appends to something. Safe only because the intent id is deduplicated
    /// server-side; without that, a reconnect delivers it twice.
    Append {
        /// What the dedup cache keys on.
        dedup: DedupKey,
        /// How long a dedup entry is kept, in seconds. Must outlive any
        /// plausible reconnect, or the retry it exists for arrives after it.
        ttl_secs: u32,
    },
    /// Raw bytes toward a process. **Never replayed** — re-sending the tail of
    /// a shell command is how a retry becomes a disaster. Resumption uses a
    /// consumed-offset acknowledgement instead.
    RawStream,
    /// Written at a UI omt does not own, so a successful write proves nothing.
    /// Confirmed by observing the far side record it; never retried.
    ExternallyConfirmed {
        /// How long to wait for that confirmation before reporting
        /// non-delivery, in milliseconds.
        confirm_within_ms: u32,
    },
    /// Last write wins, with a visible loser. Correctness here is not
    /// exactly-once; it is that the loser is told.
    LwwFreeText,
}

impl Intent {
    /// Whether dispatch may replay a repeat instead of refusing it.
    #[must_use]
    pub const fn retry_is_safe(&self) -> bool {
        matches!(self, Self::Cas | Self::Append { .. } | Self::LwwFreeText)
    }
}

/// Which surfaces a capability is excused from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Surface {
    /// The native TUI.
    Tui,
    /// The web client.
    Web,
    /// The command-line interface.
    Cli,
    /// The generated reference.
    Docs,
}

/// Whether a capability owes every surface an affordance.
///
/// Serialize-only: the surface list is `&'static [Surface]` so a `Decl` can stay
/// entirely `const`, which is what lets the catalog be link-time data with no
/// initializer to run. Nothing deserializes a declaration — they are compiled
/// in, and codegen reads them out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(tag = "parity", rename_all = "snake_case")]
pub enum Parity {
    /// Owes all of them.
    Full,
    /// Excused from the named surfaces, for the stated reason.
    ///
    /// Per-surface rather than wholesale, so a CLI-only exemption cannot
    /// silently excuse the web — which is where the exemption would actually
    /// hurt.
    Exempt {
        /// The surfaces excused.
        surfaces: &'static [Surface],
        /// Why. Printed in the generated reference, so it is argued in public.
        reason: &'static str,
    },
}

/// Everything a `capability!` declares.
///
/// All `const`, so the whole catalog is link-time data with no initializer to
/// run and nothing to fail at startup.
#[derive(Debug, Clone, Copy)]
pub struct Decl {
    /// The stable dotted id, and the wire name.
    pub name: &'static str,
    /// The CLI group, e.g. `session`.
    pub group: &'static str,
    /// The CLI verb, e.g. `send-text`.
    pub verb: &'static str,
    /// Human-readable title, used by the command palette.
    pub title: &'static str,
    /// Alternative names for palette search and the CLI.
    pub aliases: &'static [&'static str],
    /// Hidden from the palette and `--help`.
    ///
    /// A hidden capability has no palette entry, so palette membership cannot
    /// satisfy its TUI reachability — it needs a real binding or an exemption.
    pub hidden: bool,
    /// Why it is hidden. Required when `hidden`, so hiding is justified rather
    /// than habitual.
    pub hidden_reason: Option<&'static str>,
    /// Query or command.
    pub kind: Kind,
    /// The minimum role.
    pub role: Role,
    /// The maximum effects. `refine_effects` may narrow, never widen.
    pub effects: Effects,
    /// Retry semantics. `None` only for queries.
    pub intent: Option<Intent>,
    /// Parity obligation.
    pub parity: Parity,
    /// The version that introduced it.
    pub since: &'static str,
    /// One-line description for the generated reference.
    pub doc: &'static str,
}

impl Decl {
    /// The REST path this declaration generates.
    #[must_use]
    pub fn route(&self) -> String {
        format!("/v1/{}/{}", self.group, self.verb)
    }

    /// Whether `surface` is excused.
    #[must_use]
    pub fn is_exempt_from(&self, surface: Surface) -> bool {
        match self.parity {
            Parity::Full => false,
            Parity::Exempt { surfaces, .. } => surfaces.contains(&surface),
        }
    }

    /// Check the declaration is internally coherent.
    ///
    /// Run at registry construction, so a malformed declaration is a startup
    /// failure naming the capability rather than a surprise at 3 a.m.
    ///
    /// # Errors
    /// Returns every problem found, not just the first — an implementer fixing
    /// a declaration should see all of them in one pass.
    pub fn validate(&self) -> Result<(), Vec<DeclError>> {
        let mut errs = Vec::new();

        if self.kind == Kind::Command && self.intent.is_none() {
            errs.push(DeclError::CommandWithoutIntent { name: self.name });
        }
        if self.kind == Kind::Query && self.intent.is_some() {
            errs.push(DeclError::QueryWithIntent { name: self.name });
        }
        if self.hidden && self.hidden_reason.is_none() {
            errs.push(DeclError::HiddenWithoutReason { name: self.name });
        }
        if !self.hidden && self.title.is_empty() {
            errs.push(DeclError::MissingTitle { name: self.name });
        }
        if self.name != format!("{}.{}", self.group, self.verb.replace('-', "_")) {
            errs.push(DeclError::NameDoesNotMatchRoute {
                name: self.name,
                group: self.group,
                verb: self.verb,
            });
        }
        // A Viewer that can destroy or write is a contradiction: the role
        // exists so a shared read-only link is genuinely read-only.
        if self.role == Role::Viewer
            && self
                .effects
                .intersects(Effects::DESTRUCTIVE | Effects::WRITES_FS | Effects::WRITES_PTY)
        {
            errs.push(DeclError::ViewerWithWriteEffects { name: self.name });
        }

        if errs.is_empty() { Ok(()) } else { Err(errs) }
    }
}

/// A declaration that cannot be right.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DeclError {
    /// A command with no retry semantics.
    #[error("`{name}` is a Command with no intent class; dispatch could not know whether a repeat is safe")]
    CommandWithoutIntent {
        /// The capability.
        name: &'static str,
    },
    /// A query carrying retry semantics it does not need.
    #[error("`{name}` is a Query with an intent class; queries are safe to retry by construction")]
    QueryWithIntent {
        /// The capability.
        name: &'static str,
    },
    /// Hidden without saying why.
    #[error("`{name}` is hidden with no `hidden_reason`; hiding a capability has to be argued")]
    HiddenWithoutReason {
        /// The capability.
        name: &'static str,
    },
    /// Visible with no title, so unreachable from the palette.
    #[error("`{name}` is not hidden but has no `title`, so it cannot be found in the palette")]
    MissingTitle {
        /// The capability.
        name: &'static str,
    },
    /// The dotted name and the route disagree.
    #[error("`{name}` does not match its group and verb (`{group}` / `{verb}`); the CLI, the route and the wire name would disagree")]
    NameDoesNotMatchRoute {
        /// The capability.
        name: &'static str,
        /// Its group.
        group: &'static str,
        /// Its verb.
        verb: &'static str,
    },
    /// A read-only role that can write.
    #[error("`{name}` is Viewer but declares write or destructive effects; a shared read-only link would not be read-only")]
    ViewerWithWriteEffects {
        /// The capability.
        name: &'static str,
    },
}

impl fmt::Display for Effects {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let w = self.to_wire();
        if w.is_empty() {
            f.write_str("—")
        } else {
            f.write_str(&w.join(", "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn decl() -> Decl {
        Decl {
            name: "session.send_text",
            group: "session",
            verb: "send-text",
            title: "Send text",
            aliases: &[],
            hidden: false,
            hidden_reason: None,
            kind: Kind::Command,
            role: Role::Operator,
            effects: Effects::WRITES_PTY,
            intent: Some(Intent::RawStream),
            parity: Parity::Full,
            since: "0.1.0",
            doc: "Send text to a session as if typed.",
        }
    }

    #[test]
    fn effects_are_sorted_strings_not_an_integer() {
        let e = Effects::WRITES_PTY | Effects::DESTRUCTIVE;
        assert_eq!(e.to_wire(), ["destructive", "writes_pty"]);
        let json = serde_json::to_string(&e).expect("serialize");
        assert_eq!(json, r#"["destructive","writes_pty"]"#);
    }

    #[test]
    fn effects_round_trip() {
        let e = Effects::NETWORK | Effects::READS_FS | Effects::WRITES_FS;
        let json = serde_json::to_string(&e).expect("serialize");
        let back: Effects = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(e, back);
    }

    #[test]
    fn an_unknown_effect_is_refused_not_ignored() {
        let err = serde_json::from_str::<Effects>(r#"["writes_pty","teleports"]"#)
            .expect_err("unknown effect must be refused");
        assert!(err.to_string().contains("teleports"), "{err}");
    }

    #[test]
    fn a_command_without_intent_is_rejected() {
        let mut d = decl();
        d.intent = None;
        let errs = d.validate().expect_err("must be rejected");
        assert_eq!(errs, [DeclError::CommandWithoutIntent { name: "session.send_text" }]);
    }

    #[test]
    fn a_query_with_intent_is_rejected() {
        let mut d = decl();
        d.kind = Kind::Query;
        // intent stays Some
        let errs = d.validate().expect_err("must be rejected");
        assert!(errs.contains(&DeclError::QueryWithIntent { name: "session.send_text" }));
    }

    #[test]
    fn hidden_requires_a_reason() {
        let mut d = decl();
        d.hidden = true;
        let errs = d.validate().expect_err("must be rejected");
        assert!(errs.contains(&DeclError::HiddenWithoutReason { name: "session.send_text" }));
    }

    #[test]
    fn a_viewer_may_not_write() {
        let mut d = decl();
        d.role = Role::Viewer;
        let errs = d.validate().expect_err("must be rejected");
        assert!(errs.contains(&DeclError::ViewerWithWriteEffects { name: "session.send_text" }));
    }

    #[test]
    fn the_name_must_match_the_route() {
        let mut d = decl();
        d.name = "session.something_else";
        let errs = d.validate().expect_err("must be rejected");
        assert!(matches!(errs[0], DeclError::NameDoesNotMatchRoute { .. }));
    }

    #[test]
    fn validation_reports_every_problem_at_once() {
        let mut d = decl();
        d.intent = None;
        d.role = Role::Viewer;
        let errs = d.validate().expect_err("must be rejected");
        assert_eq!(errs.len(), 2, "an implementer should see both: {errs:?}");
    }

    #[test]
    fn a_good_declaration_validates() {
        decl().validate().expect("should be valid");
    }

    #[test]
    fn routes_derive_from_group_and_verb() {
        assert_eq!(decl().route(), "/v1/session/send-text");
    }

    #[test]
    fn exemptions_are_per_surface() {
        let mut d = decl();
        d.parity = Parity::Exempt {
            surfaces: &[Surface::Cli],
            reason: "no notification surface on a CLI",
        };
        assert!(d.is_exempt_from(Surface::Cli));
        assert!(!d.is_exempt_from(Surface::Web), "a CLI exemption must not excuse the web");
    }

    #[test]
    fn raw_stream_is_never_retryable() {
        assert!(!Intent::RawStream.retry_is_safe());
        assert!(!Intent::ExternallyConfirmed { confirm_within_ms: 10_000 }.retry_is_safe());
        assert!(Intent::Cas.retry_is_safe());
        assert!(Intent::Append { dedup: DedupKey::IntentId, ttl_secs: 600 }.retry_is_safe());
        assert!(Intent::LwwFreeText.retry_is_safe());
    }
}
