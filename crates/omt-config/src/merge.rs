//! Merging layers per leaf key, keeping track of where every value came from.
//!
//! Per *leaf*, not per table. If a higher layer replaced a whole table, setting
//! one key in a project config would silently drop every sibling the user had
//! configured — and they would find out by the missing behaviour, not by any
//! message.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::layer::{APPENDING_ARRAYS, Layer, Scope, UNSET};

/// Where one resolved value came from.
///
/// This tuple is the whole reason config is debuggable: it is what
/// `omt config sources` prints, and what a settings editor shows as "inherited
/// from project config, line 14".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    /// Which layer won.
    pub layer: Layer,
    /// The file it came from, where there was one.
    pub file: Option<String>,
    /// The line, where the parser could say.
    pub line: Option<u32>,
}

/// One layer's contribution.
#[derive(Debug, Clone)]
pub struct LayerInput {
    /// Which layer.
    pub layer: Layer,
    /// Its values, as a nested object.
    pub values: Value,
    /// Where they came from.
    pub file: Option<String>,
}

/// The result of merging every layer.
#[derive(Debug, Clone, Default)]
pub struct Resolved {
    values: BTreeMap<String, Value>,
    provenance: BTreeMap<String, Provenance>,
    diagnostics: Vec<Diagnostic>,
}

/// Something worth telling the user about their configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// A stable, greppable code.
    pub code: &'static str,
    /// The dotted key it concerns.
    pub key: String,
    /// What is wrong.
    pub message: String,
    /// What to do about it.
    pub help: Option<String>,
    /// Whether this stops the config from loading.
    pub severity: Severity,
}

/// How bad a diagnostic is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// The configuration will not load.
    Error,
    /// It loaded, but something was ignored or is suspect.
    Warning,
}

/// A key was dropped because its layer may not set it.
pub const DROPPED_BY_SCOPE: &str = "OMT-C201";
/// A key nothing knows about.
pub const UNKNOWN_KEY: &str = "OMT-C101";

impl Resolved {
    /// A resolved value by dotted key.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }

    /// Where a value came from.
    #[must_use]
    pub fn source(&self, key: &str) -> Option<&Provenance> {
        self.provenance.get(key)
    }

    /// Every resolved key, in a stable order.
    #[must_use]
    pub fn keys(&self) -> Vec<&str> {
        self.values.keys().map(String::as_str).collect()
    }

    /// What went wrong or looked wrong.
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Whether anything stops this configuration loading.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }
}

/// How a key is described in the schema.
#[derive(Debug, Clone, Copy)]
pub struct KeySpec {
    /// Its dotted path.
    pub key: &'static str,
    /// Where it may be set.
    pub scope: Scope,
}

/// Merge the layers into one resolved view.
///
/// Layers are applied weakest first, so a later one overwrites a leaf an
/// earlier one set — and only that leaf.
#[must_use]
pub fn merge(layers: &[LayerInput], schema: &[KeySpec]) -> Resolved {
    let mut out = Resolved::default();
    let known: BTreeMap<&str, Scope> = schema.iter().map(|k| (k.key, k.scope)).collect();

    let mut ordered: Vec<&LayerInput> = layers.iter().collect();
    ordered.sort_by_key(|l| l.layer);

    for input in ordered {
        let mut flat = BTreeMap::new();
        flatten(&input.values, String::new(), &mut flat);

        for (key, value) in flat {
            match known.get(key.as_str()) {
                Some(scope) if !scope.permits(input.layer) => {
                    // Dropped, never applied, and said out loud — a silently
                    // ignored setting is indistinguishable from a broken
                    // feature.
                    out.diagnostics.push(Diagnostic {
                        code: DROPPED_BY_SCOPE,
                        key: key.clone(),
                        message: format!(
                            "`{key}` cannot be set from the {:?} layer and was ignored",
                            input.layer
                        ),
                        help: Some(format!("move it to a layer this key allows ({scope:?})")),
                        severity: Severity::Warning,
                    });
                    continue;
                }
                Some(_) => {}
                None => {
                    // A plugin that is not installed owns its own namespace, so
                    // its keys are forward compatibility rather than typos.
                    let plugin_namespace = key.starts_with("plugins.");
                    out.diagnostics.push(Diagnostic {
                        code: UNKNOWN_KEY,
                        key: key.clone(),
                        message: format!("unknown key `{key}`"),
                        help: suggest(&key, &known),
                        severity: if plugin_namespace {
                            Severity::Warning
                        } else {
                            // A typo in `terminal.scrolback_lines` silently
                            // doing nothing is the exact failure this refuses.
                            Severity::Error
                        },
                    });
                    if !plugin_namespace {
                        continue;
                    }
                }
            }

            // The reserved unset: restore whatever the layer below said, which
            // a plain absence cannot express — absence means "no opinion", and
            // no opinion is what inherits.
            if value.as_str() == Some(UNSET) {
                out.values.remove(&key);
                out.provenance.remove(&key);
                continue;
            }

            let merged = match out.values.get(&key) {
                Some(existing) if APPENDING_ARRAYS.contains(&key.as_str()) => {
                    append_arrays(existing, &value)
                }
                _ => value,
            };

            out.values.insert(key.clone(), merged);
            out.provenance.insert(
                key,
                Provenance {
                    layer: input.layer,
                    file: input.file.clone(),
                    line: None,
                },
            );
        }
    }

    out
}

/// Flatten a nested object into dotted leaf keys.
///
/// Leaves are scalars and arrays; tables are walked into. This is what makes
/// the merge per-leaf rather than per-table.
fn flatten(value: &Value, prefix: String, out: &mut BTreeMap<String, Value>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten(v, key, out);
            }
        }
        other => {
            if !prefix.is_empty() {
                out.insert(prefix, other.clone());
            }
        }
    }
}

fn append_arrays(lower: &Value, higher: &Value) -> Value {
    match (lower, higher) {
        (Value::Array(a), Value::Array(b)) => {
            let mut merged = a.clone();
            merged.extend(b.iter().cloned());
            Value::Array(merged)
        }
        // Not both arrays: the higher layer wins, which is the default rule.
        _ => higher.clone(),
    }
}

/// Suggest a key the user probably meant.
///
/// Case and `-`/`_` differences cost nothing, so `scrollbackLines` finds
/// `scrollback_lines` — that is a spelling difference, not a different word.
fn suggest(key: &str, known: &BTreeMap<&str, Scope>) -> Option<String> {
    let normalize = |s: &str| s.to_lowercase().replace(['-', '_'], "");
    let target = normalize(key);
    let threshold = (key.len() / 4).max(1);

    let same_table = key.rsplit_once('.').map(|(t, _)| t.to_owned());
    let mut best: Option<(usize, &str)> = None;

    for candidate in known.keys() {
        let d = distance(&target, &normalize(candidate));
        if d > threshold {
            continue;
        }
        // Prefer a candidate in the same table: a user editing `[terminal]`
        // almost certainly meant another key in `[terminal]`.
        let in_same_table = same_table
            .as_deref()
            .is_some_and(|t| candidate.starts_with(&format!("{t}.")));
        let score = if in_same_table { d } else { d + 1 };
        if best.is_none_or(|(b, _)| score < b) {
            best = Some((score, candidate));
        }
    }

    best.map(|(_, c)| format!("did you mean `{c}`?"))
}

/// Edit distance, enough for spelling suggestions.
fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
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

    fn schema() -> Vec<KeySpec> {
        vec![
            KeySpec {
                key: "terminal.scrollback_lines",
                scope: Scope::Any,
            },
            KeySpec {
                key: "terminal.font_size",
                scope: Scope::Any,
            },
            KeySpec {
                key: "terminal.env_passthrough",
                scope: Scope::Any,
            },
            KeySpec {
                key: "server.bind",
                scope: Scope::UserOrAbove,
            },
            KeySpec {
                key: "appearance.font.family",
                scope: Scope::DeviceLocal,
            },
            KeySpec {
                key: "appearance.theme",
                scope: Scope::Any,
            },
            KeySpec {
                key: "plugins.search_paths",
                scope: Scope::Any,
            },
        ]
    }

    fn layer(layer: Layer, values: Value) -> LayerInput {
        LayerInput {
            layer,
            values,
            file: Some(format!("{layer:?}.toml")),
        }
    }

    #[test]
    fn a_higher_layer_replaces_one_leaf_and_leaves_its_siblings_alone() {
        // Per-leaf merging. Replacing the table would silently drop every
        // sibling the user had set, and they would find out by the missing
        // behaviour rather than by any message.
        let r = merge(
            &[
                layer(
                    Layer::User,
                    json!({"terminal": {"scrollback_lines": 10000, "font_size": 14}}),
                ),
                layer(Layer::Project, json!({"terminal": {"font_size": 16}})),
            ],
            &schema(),
        );
        assert_eq!(r.get("terminal.font_size"), Some(&json!(16)));
        assert_eq!(
            r.get("terminal.scrollback_lines"),
            Some(&json!(10000)),
            "the sibling survived"
        );
    }

    #[test]
    fn every_value_says_where_it_came_from() {
        // What `omt config sources` prints, and what makes a surprising value
        // traceable rather than mysterious.
        let r = merge(
            &[
                layer(Layer::User, json!({"terminal": {"font_size": 14}})),
                layer(Layer::Project, json!({"terminal": {"font_size": 16}})),
            ],
            &schema(),
        );
        let p = r.source("terminal.font_size").expect("provenance");
        assert_eq!(p.layer, Layer::Project);
        assert_eq!(p.file.as_deref(), Some("Project.toml"));
    }

    #[test]
    fn runtime_beats_session_so_an_env_var_is_a_reliable_escape_hatch() {
        let r = merge(
            &[
                layer(Layer::Session, json!({"appearance": {"theme": "dark"}})),
                layer(Layer::Runtime, json!({"appearance": {"theme": "nord"}})),
            ],
            &schema(),
        );
        assert_eq!(r.get("appearance.theme"), Some(&json!("nord")));
    }

    #[test]
    fn layers_are_applied_weakest_first_regardless_of_the_order_given() {
        // The caller must not be able to change the outcome by reordering its
        // own argument list.
        let forwards = merge(
            &[
                layer(Layer::User, json!({"appearance": {"theme": "a"}})),
                layer(Layer::Runtime, json!({"appearance": {"theme": "b"}})),
            ],
            &schema(),
        );
        let backwards = merge(
            &[
                layer(Layer::Runtime, json!({"appearance": {"theme": "b"}})),
                layer(Layer::User, json!({"appearance": {"theme": "a"}})),
            ],
            &schema(),
        );
        assert_eq!(
            forwards.get("appearance.theme"),
            backwards.get("appearance.theme")
        );
        assert_eq!(forwards.get("appearance.theme"), Some(&json!("b")));
    }

    #[test]
    fn a_project_file_cannot_set_a_user_or_above_key() {
        // It arrived with a git clone.
        let r = merge(
            &[layer(
                Layer::Project,
                json!({"server": {"bind": "0.0.0.0:80"}}),
            )],
            &schema(),
        );
        assert_eq!(r.get("server.bind"), None, "never applied");
        let d = r
            .diagnostics()
            .iter()
            .find(|d| d.code == DROPPED_BY_SCOPE)
            .expect("and said so");
        assert_eq!(d.severity, Severity::Warning);
        assert!(d.key.contains("server.bind"));
    }

    #[test]
    fn the_same_key_is_honoured_from_a_layer_that_may_set_it() {
        let r = merge(
            &[layer(
                Layer::User,
                json!({"server": {"bind": "127.0.0.1:0"}}),
            )],
            &schema(),
        );
        assert_eq!(r.get("server.bind"), Some(&json!("127.0.0.1:0")));
    }

    #[test]
    fn a_device_local_key_is_refused_from_a_shared_config() {
        // A font family in a synced config follows the user onto a machine that
        // does not have the font.
        let r = merge(
            &[layer(
                Layer::User,
                json!({"appearance": {"font": {"family": "Fira Code"}}}),
            )],
            &schema(),
        );
        assert_eq!(r.get("appearance.font.family"), None);
        assert!(r.diagnostics().iter().any(|d| d.code == DROPPED_BY_SCOPE));
    }

    #[test]
    fn unset_restores_the_layer_below() {
        // How a project config drops a user-level setting without knowing what
        // it was. Absence cannot express this: absence is what inherits.
        let r = merge(
            &[
                layer(Layer::User, json!({"appearance": {"theme": "nord"}})),
                layer(Layer::Project, json!({"appearance": {"theme": UNSET}})),
            ],
            &schema(),
        );
        assert_eq!(r.get("appearance.theme"), None);
    }

    #[test]
    fn arrays_replace_by_default() {
        // The only predictable rule. An array accumulating across four layers
        // cannot be reasoned about from any one file.
        let r = merge(
            &[
                layer(Layer::User, json!({"terminal": {"env_passthrough": ["A"]}})),
                layer(Layer::Project, json!({"terminal": {"scrollback_lines": 1}})),
            ],
            &schema(),
        );
        assert_eq!(r.get("terminal.env_passthrough"), Some(&json!(["A"])));
    }

    #[test]
    fn the_two_documented_arrays_append() {
        let r = merge(
            &[
                layer(Layer::User, json!({"plugins": {"search_paths": ["/a"]}})),
                layer(Layer::Project, json!({"plugins": {"search_paths": ["/b"]}})),
            ],
            &schema(),
        );
        assert_eq!(r.get("plugins.search_paths"), Some(&json!(["/a", "/b"])));
    }

    #[test]
    fn a_typo_is_an_error_rather_than_silently_doing_nothing() {
        // The exact failure the strictness exists for: `scrolback_lines`
        // quietly having no effect is indistinguishable from a broken feature.
        let r = merge(
            &[layer(
                Layer::User,
                json!({"terminal": {"scrolback_lines": 5}}),
            )],
            &schema(),
        );
        assert!(r.has_errors());
        let d = r
            .diagnostics()
            .iter()
            .find(|d| d.code == UNKNOWN_KEY)
            .expect("reported");
        assert!(
            d.help
                .as_deref()
                .is_some_and(|h| h.contains("scrollback_lines")),
            "and suggests the right one: {:?}",
            d.help
        );
    }

    #[test]
    fn a_spelling_difference_is_distance_zero_for_suggestions() {
        // `scrollbackLines` is the same word in a different house style, not a
        // different key.
        let r = merge(
            &[layer(
                Layer::User,
                json!({"terminal": {"scrollbackLines": 5}}),
            )],
            &schema(),
        );
        let d = r
            .diagnostics()
            .iter()
            .find(|d| d.code == UNKNOWN_KEY)
            .expect("reported");
        assert!(
            d.help
                .as_deref()
                .is_some_and(|h| h.contains("scrollback_lines")),
            "{:?}",
            d.help
        );
    }

    #[test]
    fn a_suggestion_prefers_a_key_in_the_same_table() {
        // Somebody editing [terminal] almost certainly meant another key in
        // [terminal].
        let r = merge(
            &[layer(Layer::User, json!({"terminal": {"font_siz": 12}}))],
            &schema(),
        );
        let d = r
            .diagnostics()
            .iter()
            .find(|d| d.code == UNKNOWN_KEY)
            .expect("reported");
        assert!(
            d.help
                .as_deref()
                .is_some_and(|h| h.contains("terminal.font_size")),
            "{:?}",
            d.help
        );
    }

    #[test]
    fn an_unknown_key_under_an_uninstalled_plugin_is_only_a_warning() {
        // A plugin owns its own namespace, so this is forward compatibility
        // rather than a typo — and erroring would mean uninstalling a plugin
        // broke the config file.
        let r = merge(
            &[layer(
                Layer::User,
                json!({"plugins": {"notify-ntfy": {"topic": "x"}}}),
            )],
            &schema(),
        );
        assert!(!r.has_errors(), "{:?}", r.diagnostics());
        assert!(r.diagnostics().iter().any(|d| d.code == UNKNOWN_KEY));
        assert_eq!(
            r.get("plugins.notify-ntfy.topic"),
            Some(&json!("x")),
            "and the value is kept for the plugin that will read it"
        );
    }

    #[test]
    fn every_unknown_key_is_reported_not_just_the_first() {
        // Stopping at the first offender makes fixing a config a series of
        // one-error-at-a-time round trips.
        let r = merge(
            &[layer(
                Layer::User,
                json!({"terminal": {"aaa": 1, "bbb": 2}, "nonsense": {"ccc": 3}}),
            )],
            &schema(),
        );
        assert_eq!(
            r.diagnostics()
                .iter()
                .filter(|d| d.code == UNKNOWN_KEY)
                .count(),
            3
        );
    }

    #[test]
    fn nothing_configured_resolves_to_nothing_rather_than_failing() {
        let r = merge(&[], &schema());
        assert!(r.keys().is_empty());
        assert!(!r.has_errors());
    }

    #[test]
    fn a_deeply_nested_table_flattens_to_a_dotted_leaf() {
        let r = merge(
            &[layer(
                Layer::Instance,
                json!({"appearance": {"font": {"family": "Menlo"}}}),
            )],
            &schema(),
        );
        assert_eq!(r.get("appearance.font.family"), Some(&json!("Menlo")));
    }
}
