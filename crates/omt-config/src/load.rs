//! Finding the config files and stacking them up.
//!
//! The merge already knew how to combine layers; nothing knew where they were.
//! Discovery is the part with the judgement calls in it, and they are all about
//! where to *stop*: a search that walks too far reads a config from a directory
//! the user does not think of as part of this project, and one that stops too
//! early misses the monorepo case entirely.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::layer::Layer;
use crate::merge::LayerInput;

/// The per-project directory, the way `.vscode` works.
pub const PROJECT_DIR: &str = ".omt";

/// The file inside it.
pub const CONFIG_FILE: &str = "config.toml";

/// How far up the tree discovery will walk.
///
/// Bounded so a config in a home directory — or in `/` — is never picked up as
/// a *project* config for something twenty levels down. Somebody working in a
/// deeply nested path should not silently inherit settings from a directory
/// they have never thought about.
pub const MAX_WALK_DEPTH: usize = 24;

/// Where a config layer's file lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located {
    /// Which layer it is.
    pub layer: Layer,
    /// Where it is.
    pub path: PathBuf,
    /// Whether it is actually there.
    ///
    /// A path that does not exist is still reported, because "omt looked here
    /// and found nothing" is what somebody debugging a missing setting needs
    /// to see. Silently omitting it leaves them guessing at the search order.
    pub exists: bool,
}

/// Why loading failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LoadError {
    /// A file exists but is not valid TOML.
    ///
    /// Refused rather than skipped: skipping it would start omt with settings
    /// silently different from what the file says, which is worse than not
    /// starting.
    #[error("`{path}` is not valid TOML: {detail}")]
    Malformed {
        /// Which file.
        path: String,
        /// What the parser said.
        detail: String,
    },
    /// A file exists but could not be read.
    #[error("could not read `{path}`: {detail}")]
    Unreadable {
        /// Which file.
        path: String,
        /// What happened.
        detail: String,
    },
}

/// Find the project config for a directory, walking up.
///
/// Stops at a `.git` boundary — but takes **one more step** past it, because a
/// monorepo keeps its shared config above the sub-repository and a search that
/// stopped dead at `.git` would never see it.
#[must_use]
pub fn find_project_config(start: &Path) -> Option<PathBuf> {
    // Two passes, because the boundary cannot be known on the way up. A
    // monorepo has `.git` at its root and often another inside a submodule, so
    // "stop at the first `.git`" misses the shared config the whole repository
    // is meant to share. The rule that works for both is: never walk above the
    // *outermost* repository — that directory is the edge of "this project",
    // and anything above it belongs to whatever the user happens to keep their
    // repositories in.
    let mut chain: Vec<&Path> = Vec::new();
    let mut dir = Some(start);
    for _ in 0..MAX_WALK_DEPTH {
        let Some(current) = dir else { break };
        chain.push(current);
        dir = current.parent();
    }

    let boundary = chain
        .iter()
        .rposition(|d| d.join(".git").exists())
        .unwrap_or(chain.len().saturating_sub(1));

    chain
        .iter()
        .take(boundary + 1)
        .map(|d| d.join(PROJECT_DIR).join(CONFIG_FILE))
        .find(|c| c.is_file())
}

/// Every place omt will look, in precedence order.
///
/// Returned whether or not the files exist, so `omt config sources` can show
/// the whole search rather than only what was found.
#[must_use]
pub fn search_paths(config_home: &Path, workspace: Option<&Path>) -> Vec<Located> {
    let mut out = vec![Located {
        layer: Layer::User,
        path: config_home.join(CONFIG_FILE),
        exists: config_home.join(CONFIG_FILE).is_file(),
    }];
    if let Some(dir) = workspace {
        let project =
            find_project_config(dir).unwrap_or_else(|| dir.join(PROJECT_DIR).join(CONFIG_FILE));
        out.push(Located {
            exists: project.is_file(),
            layer: Layer::Project,
            path: project,
        });
    }
    out.push(Located {
        layer: Layer::Instance,
        path: config_home.join("instances.toml"),
        exists: config_home.join("instances.toml").is_file(),
    });
    out
}

/// Read every layer that exists.
///
/// A file that is missing contributes nothing, which is different from a file
/// that is broken — the first is the normal case and the second stops the load.
///
/// # Errors
/// Fails if a file exists but cannot be read or parsed.
pub fn load(config_home: &Path, workspace: Option<&Path>) -> Result<Vec<LayerInput>, LoadError> {
    let mut out = Vec::new();
    for located in search_paths(config_home, workspace) {
        if !located.exists {
            continue;
        }
        let text = std::fs::read_to_string(&located.path).map_err(|e| LoadError::Unreadable {
            path: located.path.display().to_string(),
            detail: e.to_string(),
        })?;
        out.push(LayerInput {
            layer: located.layer,
            values: parse_toml(&text, &located.path)?,
            file: Some(located.path.display().to_string()),
        });
    }
    Ok(out)
}

/// Environment variables as the runtime layer.
///
/// `OMT_APPEARANCE__THEME=nord` becomes `appearance.theme`. Double underscore
/// for the separator, because a single one is a legitimate part of a key name
/// and `OMT_SCROLLBACK_LINES` would otherwise be ambiguous.
#[must_use]
pub fn from_environment<I>(vars: I) -> LayerInput
where
    I: IntoIterator<Item = (String, String)>,
{
    let mut object = serde_json::Map::new();
    for (key, value) in vars {
        let Some(rest) = key.strip_prefix("OMT_") else {
            continue;
        };
        let dotted = rest.to_lowercase().replace("__", ".");
        insert_dotted(&mut object, &dotted, infer(&value));
    }
    LayerInput {
        layer: Layer::Runtime,
        values: Value::Object(object),
        file: Some("the environment".to_owned()),
    }
}

/// Read a value as the type it looks like.
///
/// An environment variable is always a string, but `OMT_TERMINAL__SCROLLBACK`
/// is a number and a config layer that stored it as `"50000"` would fail every
/// type check downstream.
fn infer(value: &str) -> Value {
    if let Ok(b) = value.parse::<bool>() {
        return Value::Bool(b);
    }
    if let Ok(i) = value.parse::<i64>() {
        return Value::from(i);
    }
    if let Ok(f) = value.parse::<f64>() {
        return Value::from(f);
    }
    Value::String(value.to_owned())
}

fn insert_dotted(object: &mut serde_json::Map<String, Value>, key: &str, value: Value) {
    match key.split_once('.') {
        None => {
            object.insert(key.to_owned(), value);
        }
        Some((head, tail)) => {
            let entry = object
                .entry(head.to_owned())
                .or_insert_with(|| Value::Object(serde_json::Map::new()));
            if let Some(nested) = entry.as_object_mut() {
                insert_dotted(nested, tail, value);
            }
        }
    }
}

fn parse_toml(text: &str, path: &Path) -> Result<Value, LoadError> {
    let doc: toml_edit::DocumentMut =
        text.parse()
            .map_err(|e: toml_edit::TomlError| LoadError::Malformed {
                path: path.display().to_string(),
                detail: e.to_string(),
            })?;
    Ok(toml_to_json(doc.as_item()))
}

fn toml_to_json(item: &toml_edit::Item) -> Value {
    match item {
        toml_edit::Item::Value(v) => value_to_json(v),
        toml_edit::Item::Table(t) => {
            let mut map = serde_json::Map::new();
            for (k, v) in t.iter() {
                map.insert(k.to_owned(), toml_to_json(v));
            }
            Value::Object(map)
        }
        toml_edit::Item::ArrayOfTables(a) => Value::Array(
            a.iter()
                .map(|t| toml_to_json(&toml_edit::Item::Table(t.clone())))
                .collect(),
        ),
        toml_edit::Item::None => Value::Null,
    }
}

fn value_to_json(value: &toml_edit::Value) -> Value {
    match value {
        toml_edit::Value::String(s) => Value::String(s.value().clone()),
        toml_edit::Value::Integer(i) => Value::from(*i.value()),
        toml_edit::Value::Float(f) => Value::from(*f.value()),
        toml_edit::Value::Boolean(b) => Value::Bool(*b.value()),
        toml_edit::Value::Datetime(d) => Value::String(d.value().to_string()),
        toml_edit::Value::Array(a) => Value::Array(a.iter().map(value_to_json).collect()),
        toml_edit::Value::InlineTable(t) => {
            let mut map = serde_json::Map::new();
            for (k, v) in t.iter() {
                map.insert(k.to_owned(), value_to_json(v));
            }
            Value::Object(map)
        }
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

    fn write(dir: &Path, rel: &str, contents: &str) {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, contents).expect("write");
    }

    #[test]
    fn a_project_config_is_found_beside_the_workspace() {
        // The `.vscode` shape: a directory in the project the user edits.
        let d = tempfile::tempdir().expect("tempdir");
        write(
            d.path(),
            ".omt/config.toml",
            "[terminal]\nscrollback_lines = 5\n",
        );
        assert_eq!(
            find_project_config(d.path()),
            Some(d.path().join(".omt").join("config.toml"))
        );
    }

    #[test]
    fn it_is_found_from_a_subdirectory() {
        // Somebody in `src/deep/nested` is still in the project.
        let d = tempfile::tempdir().expect("tempdir");
        write(d.path(), ".omt/config.toml", "x = 1\n");
        std::fs::create_dir_all(d.path().join("src/deep/nested")).expect("mkdir");
        assert!(find_project_config(&d.path().join("src/deep/nested")).is_some());
    }

    #[test]
    fn a_monorepo_shared_config_is_found_past_a_submodules_git() {
        // A monorepo keeps its shared config above the sub-repository, and a
        // search that stopped dead at `.git` would never see it.
        let d = tempfile::tempdir().expect("tempdir");
        write(d.path(), ".omt/config.toml", "shared = true\n");
        // A monorepo: `.git` at the root, and another inside a submodule. A
        // search that stopped at the first one would miss the shared config
        // the whole repository exists to share.
        std::fs::create_dir_all(d.path().join(".git")).expect("mkdir");
        std::fs::create_dir_all(d.path().join("packages/api/.git")).expect("mkdir");
        std::fs::create_dir_all(d.path().join("packages/api/src")).expect("mkdir");

        assert!(
            find_project_config(&d.path().join("packages/api/src")).is_some(),
            "the monorepo's shared config was not found"
        );
    }

    #[test]
    fn the_walk_never_goes_above_the_outermost_repository() {
        // A config above the repository belongs to whatever the user keeps
        // their repositories in, not to this project. Inheriting it silently is
        // the failure this bound exists for.
        let d = tempfile::tempdir().expect("tempdir");
        write(d.path(), ".omt/config.toml", "far_away = true\n");
        std::fs::create_dir_all(d.path().join("repos/project/.git")).expect("mkdir");
        std::fs::create_dir_all(d.path().join("repos/project/src")).expect("mkdir");

        assert_eq!(
            find_project_config(&d.path().join("repos/project/src")),
            None,
            "a config two levels above the repository was inherited"
        );
    }

    #[test]
    fn a_nearer_config_wins_over_a_farther_one() {
        let d = tempfile::tempdir().expect("tempdir");
        write(d.path(), ".omt/config.toml", "which = \"outer\"\n");
        write(d.path(), "inner/.omt/config.toml", "which = \"inner\"\n");
        let found = find_project_config(&d.path().join("inner")).expect("found");
        assert!(found.to_string_lossy().contains("inner"), "{found:?}");
    }

    #[test]
    fn the_search_reports_where_it_looked_even_when_nothing_was_there() {
        // "omt looked here and found nothing" is what somebody debugging a
        // missing setting needs to see.
        let d = tempfile::tempdir().expect("tempdir");
        let paths = search_paths(d.path(), Some(d.path()));
        assert!(paths.iter().any(|p| p.layer == Layer::User));
        assert!(paths.iter().any(|p| p.layer == Layer::Project));
        assert!(paths.iter().all(|p| !p.exists), "nothing exists yet");
    }

    #[test]
    fn the_search_is_in_precedence_order() {
        // So a listing reads the way the merge behaves.
        let d = tempfile::tempdir().expect("tempdir");
        let layers: Vec<Layer> = search_paths(d.path(), Some(d.path()))
            .iter()
            .map(|p| p.layer)
            .collect();
        let mut sorted = layers.clone();
        sorted.sort_unstable();
        assert_eq!(layers, sorted);
    }

    #[test]
    fn a_project_config_overrides_the_user_one_per_leaf() {
        // The whole point of layering: setting one key in a project must not
        // drop the siblings the user configured.
        let home = tempfile::tempdir().expect("home");
        let project = tempfile::tempdir().expect("project");
        write(
            home.path(),
            "config.toml",
            "[terminal]\nscrollback_lines = 10000\nfont_size = 14\n",
        );
        write(
            project.path(),
            ".omt/config.toml",
            "[terminal]\nfont_size = 16\n",
        );

        let layers = load(home.path(), Some(project.path())).expect("load");
        let schema = [
            crate::merge::KeySpec {
                key: "terminal.scrollback_lines",
                scope: crate::Scope::Any,
            },
            crate::merge::KeySpec {
                key: "terminal.font_size",
                scope: crate::Scope::Any,
            },
        ];
        let resolved = crate::merge(&layers, &schema);
        assert_eq!(
            resolved.get("terminal.font_size"),
            Some(&serde_json::json!(16))
        );
        assert_eq!(
            resolved.get("terminal.scrollback_lines"),
            Some(&serde_json::json!(10000)),
            "the sibling the user set was dropped"
        );
        assert_eq!(
            resolved.source("terminal.font_size").map(|p| p.layer),
            Some(Layer::Project)
        );
    }

    #[test]
    fn a_missing_file_contributes_nothing_rather_than_failing() {
        // The normal case: most people have no project config.
        let d = tempfile::tempdir().expect("tempdir");
        assert!(load(d.path(), Some(d.path())).expect("load").is_empty());
    }

    #[test]
    fn a_broken_file_stops_the_load_rather_than_being_skipped() {
        // Skipping it would start omt with settings silently different from
        // what the file says, which is worse than not starting.
        let d = tempfile::tempdir().expect("tempdir");
        write(d.path(), "config.toml", "[terminal\nbroken");
        let err = load(d.path(), None).expect_err("must refuse");
        assert!(matches!(err, LoadError::Malformed { .. }), "{err:?}");
    }

    #[test]
    fn the_environment_becomes_the_runtime_layer() {
        let layer = from_environment([
            ("OMT_APPEARANCE__THEME".to_owned(), "nord".to_owned()),
            ("PATH".to_owned(), "/usr/bin".to_owned()),
        ]);
        assert_eq!(layer.layer, Layer::Runtime);
        assert_eq!(layer.values["appearance"]["theme"], "nord");
        assert!(
            layer.values.get("path").is_none(),
            "an unrelated var leaked"
        );
    }

    #[test]
    fn an_environment_value_becomes_the_type_it_looks_like() {
        // An env var is always a string, but a config layer storing "50000"
        // for a number fails every type check downstream.
        let layer = from_environment([
            ("OMT_TERMINAL__SCROLLBACK".to_owned(), "50000".to_owned()),
            ("OMT_TERMINAL__LIGATURES".to_owned(), "true".to_owned()),
            ("OMT_APPEARANCE__THEME".to_owned(), "nord".to_owned()),
        ]);
        assert_eq!(layer.values["terminal"]["scrollback"], 50_000);
        assert_eq!(layer.values["terminal"]["ligatures"], true);
        assert_eq!(layer.values["appearance"]["theme"], "nord");
    }

    #[test]
    fn the_environment_beats_every_file() {
        // The escape hatch: an operator saying "for this process, now" has to
        // win, or an env var cannot unstick a broken instance.
        let home = tempfile::tempdir().expect("home");
        write(
            home.path(),
            "config.toml",
            "[appearance]\ntheme = \"from-file\"\n",
        );
        let mut layers = load(home.path(), None).expect("load");
        layers.push(from_environment([(
            "OMT_APPEARANCE__THEME".to_owned(),
            "from-env".to_owned(),
        )]));

        let schema = [crate::merge::KeySpec {
            key: "appearance.theme",
            scope: crate::Scope::Any,
        }];
        assert_eq!(
            crate::merge(&layers, &schema).get("appearance.theme"),
            Some(&serde_json::json!("from-env"))
        );
    }

    #[test]
    fn toml_shapes_survive_the_conversion() {
        // Arrays and nested tables both reach the merge intact, or a setting
        // would be dropped between reading it and using it.
        let d = tempfile::tempdir().expect("tempdir");
        write(
            d.path(),
            "config.toml",
            "[appearance.font]\nfamily = \"Menlo\"\nfallback = [\"A\", \"B\"]\nsize = 13.5\n",
        );
        let layers = load(d.path(), None).expect("load");
        let values = &layers[0].values;
        assert_eq!(values["appearance"]["font"]["family"], "Menlo");
        assert_eq!(values["appearance"]["font"]["fallback"][1], "B");
        assert_eq!(values["appearance"]["font"]["size"], 13.5);
    }
}
