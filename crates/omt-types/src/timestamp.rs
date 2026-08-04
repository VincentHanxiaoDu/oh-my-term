//! Wall-clock instants, as they appear on the wire.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A wall-clock instant.
///
/// A newtype rather than a bare `OffsetDateTime` so the wire format is fixed
/// here, once: **RFC 3339**, which every client language parses and a human can
/// read in a log without a decoder ring. Leaving it to each `serde` attribute
/// would let one crate emit epoch millis and another emit a string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(time::OffsetDateTime);

impl Timestamp {
    /// The Unix epoch. Used by tests that need a fixed instant.
    pub const UNIX_EPOCH: Self = Self(time::OffsetDateTime::UNIX_EPOCH);

    /// Now, in UTC.
    #[must_use]
    pub fn now() -> Self {
        Self(time::OffsetDateTime::now_utc())
    }

    /// Wrap an existing instant.
    #[must_use]
    pub const fn from_offset(dt: time::OffsetDateTime) -> Self {
        Self(dt)
    }

    /// The underlying instant.
    #[must_use]
    pub const fn as_offset(&self) -> time::OffsetDateTime {
        self.0
    }

    /// Milliseconds since the epoch, for arithmetic that wants a number.
    #[must_use]
    pub fn unix_millis(&self) -> i128 {
        self.0.unix_timestamp_nanos() / 1_000_000
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0.format(&time::format_description::well_known::Rfc3339) {
            Ok(s) => f.write_str(&s),
            Err(_) => f.write_str("<unformattable timestamp>"),
        }
    }
}

impl Serialize for Timestamp {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let text = self
            .0
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(serde::ser::Error::custom)?;
        s.serialize_str(&text)
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        time::OffsetDateTime::parse(&text, &time::format_description::well_known::Rfc3339)
            .map(Self)
            .map_err(serde::de::Error::custom)
    }
}

impl schemars::JsonSchema for Timestamp {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Timestamp".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "format": "date-time",
            "description": "An RFC 3339 instant.",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wire_format_is_rfc_3339() {
        let json = serde_json::to_string(&Timestamp::UNIX_EPOCH).expect("serialize");
        assert_eq!(json, r#""1970-01-01T00:00:00Z""#);
    }

    #[test]
    fn timestamps_round_trip() {
        let t = Timestamp::now();
        let json = serde_json::to_string(&t).expect("serialize");
        let back: Timestamp = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(t.unix_millis(), back.unix_millis());
    }

    #[test]
    fn a_non_rfc3339_string_is_refused() {
        serde_json::from_str::<Timestamp>(r#""yesterday""#)
            .expect_err("must refuse an unparseable instant");
    }
}
