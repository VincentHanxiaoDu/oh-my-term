//! Reading `omt-plugin.toml`.
//!
//! `docs/architecture/11-plugins.md` §2. Three rules, each of which exists
//! because the obvious alternative fails somebody quietly.
//!
//! **`omt_api` is required, and a manifest without it is refused outright with
//! the error naming the field.** A plugin that does not say which host it was
//! written against is one omt would have to guess about, and the guess is wrong
//! exactly when the API has moved — which is the case the field exists for.
//!
//! **Unknown top-level keys are warnings; unknown keys inside a known table are
//! errors.** A new top-level table is how a plugin targets a newer omt, so
//! refusing it would make forward compatibility impossible. Inside a table omt
//! already understands there is no such story: an unrecognised key there is a
//! typo, and accepting it silently means the setting the author thought they
//! wrote is not in effect.
//!
//! **A manifest that fails to parse does not remove the plugin from the
//! registry.** It keeps its entry with `Status::Invalid` and its diagnostics, so
//! the user is told *why* their plugin vanished rather than watching it vanish.

use omt_config::merge::{Diagnostic, Severity};

/// The manifest is missing a required field.
pub const MISSING_FIELD: &str = "OMT-P101";
/// A key nothing inside a known table knows about.
pub const UNKNOWN_KEY: &str = "OMT-P102";
/// A key omt does not know at the top level.
pub const UNKNOWN_TABLE: &str = "OMT-P103";
/// The file is not TOML at all.
pub const NOT_TOML: &str = "OMT-P104";
/// A value of the wrong shape.
pub const BAD_VALUE: &str = "OMT-P105";
/// The host is outside the plugin's declared range.
pub const API_MISMATCH: &str = "OMT-P106";

/// Whether a plugin's manifest is usable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// It parsed and is complete.
    Valid,
    /// It did not, and the entry is kept to say so.
    Invalid,
}

/// What reading a manifest produced.
#[derive(Debug, Clone)]
pub struct Read {
    /// Whether it is usable.
    pub status: Status,
    /// What the plugin declared, when enough of it parsed to say.
    pub id: Option<String>,
    /// The version range of omt it was written against.
    pub omt_api: Option<String>,
    /// Everything worth telling the user.
    pub diagnostics: Vec<Diagnostic>,
}

impl Read {
    /// The diagnostics that stop it loading.
    #[must_use]
    pub fn errors(&self) -> Vec<&Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect()
    }
}

/// Top-level keys this host understands.
const KNOWN_TOP_LEVEL: &[&str] = &[
    "id",
    "name",
    "version",
    "description",
    "authors",
    "license",
    "homepage",
    "omt_api",
    "platforms",
    "entrypoint",
    "build",
    "permissions",
    "limits",
    "contributes",
    "config_schema",
];

/// Keys understood inside each known table.
fn known_keys(table: &str) -> Option<&'static [&'static str]> {
    match table {
        "entrypoint" => Some(&["kind", "command", "module", "restart", "max_restarts"]),
        "build" => Some(&["command", "timeout"]),
        "permissions" => Some(&[
            "capabilities",
            "events",
            "role",
            "fs",
            "net",
            "exec",
            "env",
        ]),
        "limits" => Some(&["memory", "cpu", "timeout", "queue"]),
        "config_schema" => Some(&["path"]),
        _ => None,
    }
}

fn diagnostic(
    code: &'static str,
    key: &str,
    message: impl Into<String>,
    help: Option<&str>,
    severity: Severity,
) -> Diagnostic {
    Diagnostic {
        code,
        key: key.to_owned(),
        message: message.into(),
        help: help.map(str::to_owned),
        severity,
    }
}

/// Read a manifest.
///
/// Never fails: an unreadable manifest is a `Read` with `Status::Invalid` and
/// the reason, because a plugin that disappears without explanation is worse
/// than one that is visibly broken.
#[must_use]
pub fn read(text: &str) -> Read {
    let document = match text.parse::<toml_edit::DocumentMut>() {
        Ok(document) => document,
        Err(error) => {
            return Read {
                status: Status::Invalid,
                id: None,
                omt_api: None,
                diagnostics: vec![diagnostic(
                    NOT_TOML,
                    "",
                    format!("the manifest is not valid TOML: {error}"),
                    Some("the plugin is kept in the registry as invalid, not removed"),
                    Severity::Error,
                )],
            };
        }
    };

    let mut diagnostics = Vec::new();
    let string_at = |key: &str| {
        document
            .get(key)
            .and_then(|item| item.as_str())
            .map(str::to_owned)
    };

    for (key, _) in document.iter() {
        if !KNOWN_TOP_LEVEL.contains(&key) {
            // Forward compatibility: this is how a plugin targets a newer omt.
            diagnostics.push(diagnostic(
                UNKNOWN_TABLE,
                key,
                format!("`{key}` is not something this omt understands"),
                Some("ignored; it may belong to a newer omt"),
                Severity::Warning,
            ));
            continue;
        }
        let Some(known) = known_keys(key) else {
            continue;
        };
        let Some(table) = document.get(key).and_then(|item| item.as_table()) else {
            continue;
        };
        for (inner, _) in table.iter() {
            if !known.contains(&inner) {
                // No forward-compatibility story inside a table omt already
                // understands: this is a typo, and the setting the author
                // thought they wrote is not in effect.
                diagnostics.push(diagnostic(
                    UNKNOWN_KEY,
                    &format!("{key}.{inner}"),
                    format!("`{inner}` is not a key of `{key}`"),
                    Some("check the spelling"),
                    Severity::Error,
                ));
            }
        }
    }

    for required in ["id", "version", "omt_api"] {
        if string_at(required).is_none() {
            diagnostics.push(diagnostic(
                MISSING_FIELD,
                required,
                format!("`{required}` is required"),
                Some(if required == "omt_api" {
                    "declare which omt versions this plugin was written against"
                } else {
                    "add it to the manifest"
                }),
                Severity::Error,
            ));
        }
    }

    if document.get("entrypoint").is_none() {
        diagnostics.push(diagnostic(
            MISSING_FIELD,
            "entrypoint",
            "`entrypoint` is required",
            Some("say what omt should run, or which module to load"),
            Severity::Error,
        ));
    } else if let Some(kind) = document
        .get("entrypoint")
        .and_then(|e| e.get("kind"))
        .and_then(toml_edit::Item::as_str)
        .filter(|kind| *kind != "process" && *kind != "wasm")
    {
        {
            diagnostics.push(diagnostic(
                BAD_VALUE,
                "entrypoint.kind",
                format!("`{kind}` is not an entrypoint kind"),
                Some("`process` or `wasm`"),
                Severity::Error,
            ));
        }
    }

    let status = if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        Status::Invalid
    } else {
        Status::Valid
    };

    Read {
        status,
        id: string_at("id"),
        omt_api: string_at("omt_api"),
        diagnostics,
    }
}

/// Whether a host version satisfies a plugin's declared range.
///
/// Deliberately narrow: `>=A, <B` and nothing else. A full semver expression
/// grammar here would be a dependency and a source of manifests that mean
/// something subtly different from what their author read.
#[must_use]
pub fn satisfies(range: &str, host: &str) -> bool {
    let host = parse_version(host);
    let Some(host) = host else { return false };
    range.split(',').all(|clause| {
        let clause = clause.trim();
        let (op, rest) = if let Some(rest) = clause.strip_prefix(">=") {
            (">=", rest)
        } else if let Some(rest) = clause.strip_prefix('<') {
            ("<", rest)
        } else if let Some(rest) = clause.strip_prefix('=') {
            ("=", rest)
        } else {
            return false;
        };
        parse_version(rest.trim()).is_some_and(|bound| match op {
            ">=" => host >= bound,
            "<" => host < bound,
            _ => host == bound,
        })
    })
}

fn parse_version(text: &str) -> Option<(u32, u32, u32)> {
    let mut parts = text.trim().split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

/// Check a read manifest against the running host.
///
/// # Errors
/// Returns a diagnostic if the host is outside the declared range.
pub fn check_api(read: &Read, host: &str) -> Result<(), Diagnostic> {
    let Some(range) = read.omt_api.as_deref() else {
        return Err(diagnostic(
            MISSING_FIELD,
            "omt_api",
            "`omt_api` is required",
            Some("declare which omt versions this plugin was written against"),
            Severity::Error,
        ));
    };
    if satisfies(range, host) {
        Ok(())
    } else {
        Err(diagnostic(
            API_MISMATCH,
            "omt_api",
            format!("this plugin wants omt {range}, and this is {host}"),
            Some("upgrade omt, or the plugin"),
            Severity::Error,
        ))
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

    const MINIMAL: &str = r#"
id = "dev.example.thing"
version = "0.1.0"
omt_api = ">=1.0, <2.0"

[entrypoint]
kind = "process"
command = ["bun", "run", "index.ts"]
"#;

    fn codes(read: &Read) -> Vec<&str> {
        read.diagnostics.iter().map(|d| d.code).collect()
    }

    #[test]
    fn a_minimal_manifest_is_valid() {
        let read = read(MINIMAL);
        assert_eq!(read.status, Status::Valid, "{:?}", read.diagnostics);
        assert_eq!(read.id.as_deref(), Some("dev.example.thing"));
    }

    #[test]
    fn a_manifest_without_omt_api_is_refused_and_the_error_names_the_field() {
        // A plugin that does not say which host it was written against is one
        // omt would have to guess about, and the guess is wrong exactly when
        // the API has moved.
        let read = read(&MINIMAL.replace("omt_api = \">=1.0, <2.0\"\n", ""));
        assert_eq!(read.status, Status::Invalid);
        let named = read
            .diagnostics
            .iter()
            .find(|d| d.key == "omt_api")
            .expect("named");
        assert_eq!(named.code, MISSING_FIELD);
    }

    #[test]
    fn an_unknown_top_level_key_is_a_warning() {
        // It is how a plugin targets a newer omt, so refusing it would make
        // forward compatibility impossible.
        let read = read(&format!("{MINIMAL}\n[contributes_something_new]\nx = 1\n"));
        assert_eq!(read.status, Status::Valid);
        assert_eq!(codes(&read), vec![UNKNOWN_TABLE]);
    }

    #[test]
    fn an_unknown_key_inside_a_known_table_is_an_error() {
        // There is no forward-compatibility story here: it is a typo, and the
        // setting the author thought they wrote is not in effect.
        let read = read(&MINIMAL.replace(
            "kind = \"process\"",
            "kind = \"process\"\nrestrt = \"on-crash\"",
        ));
        assert_eq!(read.status, Status::Invalid);
        let bad = read
            .diagnostics
            .iter()
            .find(|d| d.code == UNKNOWN_KEY)
            .expect("flagged");
        assert_eq!(bad.key, "entrypoint.restrt");
    }

    #[test]
    fn a_manifest_that_will_not_parse_is_kept_as_invalid_with_a_reason() {
        // The user sees why their plugin vanished rather than it silently
        // vanishing.
        let read = read("id = \"x\"\nthis is not toml");
        assert_eq!(read.status, Status::Invalid);
        assert_eq!(codes(&read), vec![NOT_TOML]);
        assert!(!read.errors().is_empty());
    }

    #[test]
    fn every_diagnostic_carries_a_greppable_code() {
        // The same discipline as the configuration diagnostics: a message a
        // user can search for outlives the wording.
        for text in [
            MINIMAL,
            "",
            "id = \"x\"\nthis is not toml",
            &format!("{MINIMAL}\n[nope]\nx = 1\n"),
        ] {
            for d in read(text).diagnostics {
                assert!(d.code.starts_with("OMT-P"), "{}", d.code);
            }
        }
    }

    #[test]
    fn an_empty_manifest_names_every_missing_field_at_once() {
        // One round trip per missing field is a bad way to learn what a
        // manifest needs.
        let read = read("");
        let keys: Vec<_> = read.diagnostics.iter().map(|d| d.key.clone()).collect();
        for required in ["id", "version", "omt_api", "entrypoint"] {
            assert!(keys.iter().any(|k| k == required), "{required} not named");
        }
    }

    #[test]
    fn an_entrypoint_kind_outside_the_two_is_refused() {
        let read = read(&MINIMAL.replace("kind = \"process\"", "kind = \"cdylib\""));
        assert_eq!(read.status, Status::Invalid);
        assert!(codes(&read).contains(&BAD_VALUE));
    }

    #[test]
    fn a_wasm_entrypoint_is_the_same_manifest() {
        // The WASM host is a transport swap: same manifest, same schema, same
        // permission model.
        let read = read(&MINIMAL.replace(
            "kind = \"process\"\ncommand = [\"bun\", \"run\", \"index.ts\"]",
            "kind = \"wasm\"\nmodule = \"plugin.wasm\"",
        ));
        assert_eq!(read.status, Status::Valid, "{:?}", read.diagnostics);
    }

    #[test]
    fn the_host_version_is_checked_against_the_declared_range() {
        let read = read(MINIMAL);
        assert!(check_api(&read, "1.4.0").is_ok());
        let out = check_api(&read, "2.0.0").expect_err("refused");
        assert_eq!(out.code, API_MISMATCH);
        assert!(out.message.contains("2.0.0"), "it says what this host is");
    }

    #[test]
    fn a_range_omt_cannot_parse_refuses_rather_than_admits() {
        // Admitting on an unparsed range means the one manifest field that
        // exists to stop a mismatch stops nothing.
        assert!(!satisfies("^1.0", "1.2.0"));
        assert!(!satisfies("", "1.2.0"));
    }

    #[test]
    fn the_bounds_are_the_ones_a_reader_would_expect() {
        assert!(satisfies(">=1.0, <2.0", "1.0.0"), "the lower bound is inclusive");
        assert!(!satisfies(">=1.0, <2.0", "2.0.0"), "the upper is not");
        assert!(!satisfies(">=1.0, <2.0", "0.9.9"));
    }
}
